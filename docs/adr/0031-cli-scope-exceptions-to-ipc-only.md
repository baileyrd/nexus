# ADR 0031: CLI-scope Exceptions to IPC-only

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** architecture
- **Context for:** Audit finding AA-03 (see [`docs/0.1.2/architecture-adherence.md`](../../../0.1.2/architecture-adherence.md))

## Context

Invariant #3 of the Nexus microkernel architecture says **CLI, TUI, MCP server, and Tauri shell all reach storage / AI / editor / etc. through one path**:

```rust
context.ipc_call(plugin_id, command, args) -> Result<serde_json::Value>
```

The 0.1.2 architecture-adherence audit found that the rule holds for MCP / ACP / Remote / TUI / shell, but `nexus-cli`'s `Cargo.toml` carries three direct service-crate dependencies that bypass `ipc_call`:

| Dep | Used by | Why direct |
|-----|---------|-----------|
| `nexus-terminal` | `crates/nexus-cli/src/commands/term.rs` (the `nexus term env/run/shell` verbs) | PTY work is intrinsically process-local: a CLI process spawns a PTY, attaches its own stdin/stdout/resize, and dies when the child exits. There is no "session" to talk to over IPC — the PTY's lifetime is the CLI invocation's lifetime. |
| `nexus-collab` | `crates/nexus-cli/src/commands/collab.rs` (the `nexus collab serve/join/token` verbs) | `serve` *is* the relay server; routing it through IPC would mean "the CLI asks the kernel to ask a plugin to spawn a server inside the CLI process," which is structural rope. `join` builds a CLI runtime and bridges the in-process `EventBus`. `token set/clear` directly drives `nexus_security::CredentialVault` (see next row). |
| `nexus-security` | `crates/nexus-cli/src/commands/collab.rs` (`CredentialVault` for `nexus collab token`) | A short, blocking keyring read/write next to the verb that needs it. Wrapping it in an IPC handler would not change behavior; it would just move the same `keyring::Entry::set_password` call to a different file. |

All three are documented in source: `term.rs:1-28` calls out the "Phase H" migration explicitly; `collab.rs:1-16` names the architecture; `Cargo.toml:14-29` lists every dep.

### Why not just call `ipc_call`?

Each direct use is structurally process-local: a verb whose work *is* "be this server / be this PTY / read this keyring entry once at startup". IPC adds a dispatcher hop and serialisation cost for zero portability or sandboxing gain — every caller is `nexus-cli` itself, running in the same process as the would-be service plugin.

The microkernel invariant exists to enforce that *cross-frontend* surfaces (storage / AI / editor / etc.) route through one path, so the shell, TUI, MCP, and community plugins all see the same capability-gated handlers. The three deps above are not cross-frontend surfaces; they are CLI-only verbs whose service crate happens to also expose a core-plugin shape for *other* frontends (the shell uses `com.nexus.terminal::*` over IPC; community plugins talk to `com.nexus.collab::*`; the keyring is reachable via `com.nexus.security::*`). The CLI's direct path is parallel to those, not in conflict with them.

## Decision

**The three CLI-scope direct deps on `nexus-terminal`, `nexus-collab`, and `nexus-security` are *accepted exceptions* to invariant #3, not violations.**

Concretely:

1. The exceptions are scoped to `crates/nexus-cli/`. **No other frontend** (TUI, MCP, ACP, Remote, shell-bridge) may depend directly on a service crate; those routes through `ipc_call` and that is enforced by `crates/nexus-bootstrap/tests/dep_invariants.rs`.

2. A new direct dep from `nexus-cli` to a service crate is allowed only when **all** of the following hold, and the rationale is recorded in a doc comment on the first import:
   - The work is intrinsically process-local (PTY attached to this process's tty, server bound on this process's port, single keyring read at this verb's startup).
   - The same capability is *also* available to other frontends via an `ipc_call` handler in the same service crate.
   - Routing through `ipc_call` would be a syntactic move only — no behaviour, sandboxing, or capability-gating benefit.

3. The "Phase H migration plan" note in `crates/nexus-cli/src/commands/term.rs:21-29` (the future daemon-backed session manager) **does not retroactively change this ADR**. If the terminal subsystem grows a long-running session daemon, the CLI verbs will route through `com.nexus.terminal::*` for session lifecycle while still using `nexus_terminal::Session` directly for the local PTY bridge (drain into stdin/stdout). The decision rule is per-call, not per-crate.

## Consequences

### Positive

- The three exceptions stop being "documented in source comments only." Future readers asking "why is `nexus-cli` linking `nexus-terminal`?" land here.
- The dep_invariants test can grow a positive-list assertion for `nexus-cli`'s service deps without contradicting the prose.
- The boundary condition is explicit: a new direct dep from `nexus-cli` requires this ADR's three-part test, not a free hand.

### Negative

- The microkernel invariant grows a documented escape hatch. Future maintainers must read this ADR before adding a fourth direct dep.

### Neutral

- No code changes. The architecture-adherence audit's ⚠️ on invariant #3 stays ⚠️ — the exceptions are real — but `architecture-adherence.md` can now link AA-03 here as resolved-by-design.

## Alternatives considered

### A. Route the three verbs through `ipc_call`

`nexus term run` would become `ipc_call("com.nexus.terminal", "session.spawn", …)`. The handler would `Session::spawn` exactly the way `term.rs` does today, but inside a core plugin instead of in the CLI verb. Result: same syscalls, same PTY lifetime, same exit code propagation — plus one serialisation hop, plus the inability to bridge stdin into the spawned session (the kernel dispatcher does not pump bytes through a JSON-shaped call boundary). Net negative.

### B. Move the three verbs out of `nexus-cli` entirely

E.g., put `nexus collab serve` in a `nexus-collab-cli` crate that the workspace's `nexus` binary forwards into. This satisfies the invariant for `nexus-cli` itself but only by relocating the same direct deps under a different crate name. The architecture intent is about the binary, not the package layout.

### C. Make `CredentialVault` reachable only over `com.nexus.security::*`

The CLI would `ipc_call` the security plugin for keyring access. This is the smallest of the three changes (~50 LOC) but adds runtime/test complexity for a verb that already runs inside the CLI process and only touches one keyring entry. Deferred until a second non-CLI caller wants the same thing.

## References

- [`docs/0.1.2/architecture-adherence.md`](../../../0.1.2/architecture-adherence.md) §AA-03 — the audit row this ADR resolves.
- [`crates/nexus-cli/src/commands/term.rs`](../../../../crates/nexus-cli/src/commands/term.rs) — direct `nexus_terminal` use.
- [`crates/nexus-cli/src/commands/collab.rs`](../../../../crates/nexus-cli/src/commands/collab.rs) — direct `nexus_collab` + `nexus_security::CredentialVault` use.
- [`crates/nexus-bootstrap/tests/dep_invariants.rs`](../../../../crates/nexus-bootstrap/tests/dep_invariants.rs) — the structural enforcement test.
- [ADR 0004](0004-crate-boundaries-and-ownership.md) — crate-boundary intent (the invariant this ADR scopes an exception to).
