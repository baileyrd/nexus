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

## Enforcement backends — *(roadmap)*

The model is platform-agnostic; the per-OS enforcement lands in `nexus-security`:

- **Linux** — [landlock](https://docs.kernel.org/userspace-api/landlock.html) for filesystem path restrictions + seccomp to block network syscalls when `network_access` is off.
- **macOS** — a seatbelt (`sandbox_init`) profile generated from the policy.
- **Windows** — restricted tokens + job objects.

Wiring into the spawn path (`nexus-terminal`, agent tool exec) and **permissioned downloads** (explicit, approved network egress under an otherwise network-off policy) follow once the backends land.
