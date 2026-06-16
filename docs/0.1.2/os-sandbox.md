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

### Wiring into the spawn path — *(next, with care)*

Confining a *spawned child* (terminal command, agent tool) is the goal, but it carries a real hazard: applying landlock/seccomp from a `std::process::Command::pre_exec` hook runs **after `fork()` in a multithreaded parent**, where heap allocation is not async-signal-safe (another thread may hold the allocator lock at the moment of `fork`). The ruleset/BPF construction allocates. Two safe shapes:

1. **Build-in-parent, apply-in-child** — construct the landlock `RulesetCreated` and the seccomp `BpfProgram` *before* fork, move them into the `pre_exec` closure, and call only the (allocation-free) `restrict_self` / `apply_filter` syscalls in the child.
2. **Helper binary** (Codex's approach) — `exec` a small single-threaded `nexus-sandbox` helper that applies the policy to itself and then `exec`s the real argv. No fork-alloc hazard at all.

`portable-pty` (the terminal backend) does not expose `pre_exec`, so the helper-binary shape is the likely path there. **Permissioned downloads** — explicit, approved network egress under an otherwise network-off policy — layer on once a spawn path is confined.
