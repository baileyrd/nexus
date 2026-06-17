# OS process sandbox

Nexus has **three** distinct containment layers — don't confuse them:

| Layer | Scope | Lives in |
|-------|-------|----------|
| **Capabilities** | What a *plugin* may ask the kernel to do (`fs.read`, `kv.write`, …) | [`capabilities.md`](capabilities.md) |
| **WASM/iframe plugin sandbox** | Isolates *community plugin* code (wasmtime / iframe) | [`plugins/community.md`](plugins/community.md) |
| **OS process sandbox** *(this doc)* | Isolates *spawned OS processes* — shell commands, agent tools | `nexus-types::sandbox` + `nexus-security` |

This page covers the third: the operating-system-level containment applied when Nexus spawns a child process (a terminal command, an agent's tool call). It mirrors the Codex CLI's model so behaviour is familiar to operators of that tool.

## Policy model

The policy *type* is [`SandboxPolicy`](../../crates/nexus-types/src/sandbox.rs) in the leaf `nexus-types` crate (so `nexus-terminal`, `nexus-agent`, and `nexus-security` can share it without a dependency cycle — the same reasoning as `ForgePathValidator`). Three escalating modes:

| Mode | Disk read | Disk write | Network |
|------|-----------|-----------|---------|
| `read-only` *(default)* | all | none | none |
| `workspace-write` | all | cwd + `writable_roots` + temp dirs | off unless `network_access` |
| `danger-full-access` | all | all | all |

`workspace-write` carves out a **read-only `.git`** inside each workspace root, so a sandboxed process can't rewrite VCS history. The system temp dirs (`/tmp`, `$TMPDIR`) are writable unless excluded via `exclude_slash_tmp` / `exclude_tmpdir_env_var`.

Serde tags are kebab-case (`"mode": "workspace-write"`) with snake_case fields, matching the config surface. Helpers: `has_full_disk_write_access`, `has_full_network_access`, `is_unrestricted`, `writable_roots_with_cwd`, `is_path_writable`, and `permissiveness` / `is_escalation_over` for approval flows (a more permissive policy than the current one is an *escalation* that should require operator opt-in).

Path checks in the model are **lexical** — the enforcement layer canonicalizes paths (resolving symlinks) before consulting the policy.

## Enforcement backends

The model is platform-agnostic; the per-OS enforcement lives in [`nexus-security::os_sandbox`](../../crates/nexus-security/src/os_sandbox.rs). `apply_to_current_thread(policy, cwd)` confines the calling thread (and any child it then `exec`s) and reports a [`SandboxStatus`](../../crates/nexus-security/src/os_sandbox.rs) — `FullyEnforced`, `PartiallyEnforced`, `NotEnforced` (kernel lacks the backend), `Skipped` (`danger-full-access`), or `Unsupported` (no backend on this OS). Enforcement is **best-effort**: where the kernel can't enforce, callers see the status rather than a hard failure.

- **Linux filesystem** — ✅ [Landlock](https://docs.kernel.org/userspace-api/landlock.html) (ABI v1): full-disk read, write only under the workspace roots. Landlock is *grant-only*, so the `.git` read-only carve-out is not enforced at this layer (honoured by macOS seatbelt + higher-layer edit tooling).
- **Linux network** — ✅ seccomp-bpf (`block_inet_sockets`): denies `socket(AF_INET / AF_INET6 / AF_PACKET)` with `EPERM` while leaving `AF_UNIX` (local IPC) and all other syscalls untouched. Apply when `has_full_network_access()` is false. A failure here is surfaced as `SandboxError::Seccomp` (network was **not** contained), unlike the best-effort filesystem status — the caller decides whether to refuse the spawn.
- **macOS** — *(roadmap)* a seatbelt (`sandbox_init`) profile generated from the policy.
- **Windows** — *(roadmap)* restricted tokens + job objects.

Both Linux restrictions are **irreversible for the calling thread**, so apply them on the thread that will do the untrusted work (or `exec` the untrusted child). `confine_current_thread(policy, cwd)` composes both per policy — filesystem via `apply_to_current_thread`, then `block_inet_sockets` unless the policy grants network — and is the entry point a sandboxed worker applies to itself.

### Confining a spawned child — `nexus-sandbox` helper

Confining a *spawned child* (terminal command, agent tool) carries a real hazard: applying landlock/seccomp from a `std::process::Command::pre_exec` hook runs **after `fork()` in a multithreaded parent**, where heap allocation is not async-signal-safe (another thread may hold the allocator lock at the moment of `fork`), and the ruleset/BPF construction allocates.

Nexus takes Codex's **helper-binary** approach, which sidesteps the hazard entirely: [`nexus-sandbox`](../../crates/nexus-security/src/bin/nexus-sandbox.rs) is a single-threaded sidecar that `confine_current_thread`s *itself* and then `exec`s the real argv (the Landlock domain + seccomp filter survive `execve`). Build the invocation with [`sandbox_command`](../../crates/nexus-security/src/os_sandbox.rs):

```rust
use nexus_security::{sandbox_command, default_helper_path};
let helper = default_helper_path().expect("nexus-sandbox alongside the exe");
let mut cmd = sandbox_command(&helper, &policy, cwd, "rustc", ["--version"])?;
let status = cmd.status()?; // runs `rustc --version` confined by `policy`
```

`sandbox_command(helper, policy, cwd, program, args)` returns a `std::process::Command` that runs `nexus-sandbox <policy-json> <cwd> -- <program> [args…]`. The frontend-agnostic argv builder `nexus_types::sandbox_argv` and the helper locator `nexus_types::default_helper_path` live in the **leaf** `nexus-types` (re-exported from `nexus-security`), so a spawn site can wrap a command *without* linking the enforcement engine — notably `portable-pty` (the terminal backend), which has no `pre_exec`.

**Terminal adoption:** `nexus_terminal::SessionConfig` carries an opt-in `sandbox: Option<SandboxPolicy>`. When set to an enforcing policy, `Session::spawn` launches the shell *through* the helper (via `sandbox_argv`); `None` (the default) and `danger-full-access` run the shell directly, so interactive sessions are never surprise-confined. It **fails closed** — a requested policy with no locatable helper errors rather than running unconfined. The agent opts in when spawning sessions for autonomous tool execution.

### Permissioned downloads

A network-off policy denies raw sockets outright (seccomp). Rather than poke a hole in that, the [`downloads`](../../crates/nexus-security/src/downloads.rs) broker performs *specific, allowlisted* fetches on the confined process's behalf and drops the result into a writable root. `validate(request, policy, writable_roots)` (pure, fully tested) enforces the rules — downloads must be **enabled**, the scheme **https**, the host on the **allowlist**, and the destination inside a **writable root** — and `fetch` streams the validated download with a size cap.

`DownloadPolicy { enabled, allowed_hosts, max_bytes }` is **off by default** (mirroring network-off-by-default); an operator opts in and names allowed hosts.

The broker is reachable over IPC via `com.nexus.security::download` (`{ url, dest, cwd? }`), gated by the `net.http` capability *and* the `sandbox.toml` allowlist + writable-root checks. The active config is introspectable via `com.nexus.security::sandbox_policy`, loaded from [`SandboxConfig`](../../crates/nexus-security/src/sandbox_config.rs) (`.forge/sandbox.toml`, closed by default).

### What remains

The enforcement primitives, config surface, download IPC, and the opt-in terminal spawn path are complete. To fully *activate* confinement for autonomous work, the agent should pass its `SandboxPolicy` (from `sandbox.toml`) when spawning terminal sessions for tool execution — the mechanism is in place; flipping it on is a per-caller decision.
