# ADR 0027: Protocol-Host Contribution Model for LSP / DAP / MCP / ACP

**Date:** 2026-05-13 (proposed), 2026-05-14 (Phase 0a accepted)
**Status:** Accepted — Phase 0a shipped. Tracks as **BL-113** in the backlog.
**Related:** [ADR 0011](0011-active-shell-target.md) (plugin-first shell), [BL-076](../PRDs/BACKLOG_COMPLETED.md) (nexus-lsp host), [BL-081](../PRDs/BL-081-dap-debugger.md) (nexus-dap host, parked on `bl-081-dap-debugger` pending this ADR), [Hermes Agent port plan](../research/hermes-agent-implementation-plan.md) Feature 7 (ACP adapter — not yet implemented; named here so the future crate lands on the contribution model from day one).

## Context

Three Nexus core plugins host external-process protocol adapters today, and a fourth (ACP) is in the queue:

| Crate | Host plugin | Adapter config | Examples | Status |
|---|---|---|---|---|
| `nexus-lsp` | `com.nexus.lsp` | `<forge>/.forge/lsp.toml` | rust-analyzer, typescript-language-server | shipped (BL-076) |
| `nexus-mcp` | `com.nexus.mcp.host` | `<forge>/.forge/mcp.toml` | filesystem, git, custom MCP servers | shipped |
| `nexus-dap` | `com.nexus.dap` | `<forge>/.forge/dap.toml` | codelldb, debugpy, js-debug, dlv | parked branch (BL-081) |
| `nexus-acp` | `com.nexus.acp` (proposed) | `<forge>/.forge/acp.toml` (proposed) | Hermes-shaped sub-agents, external A2A peers | not built — Hermes Feature 7 |

All four follow the same shape: the *host* is a core Rust plugin registered at bootstrap; the *adapters* are external executables named in a flat TOML config. The host proxies a protocol surface (JSON-RPC for LSP/MCP/ACP, a `type`-tagged JSON envelope for DAP) over IPC and republishes server-pushed messages on the kernel bus.

ACP (Agent Communication Protocol — see the Hermes plan) is the agent-to-agent / IDE-to-agent equivalent of LSP. It's stdio JSON-RPC with a request/response/notification family; the wire shape and the connection-pool / reconnect / event-fan-out concerns are all near-identical to what `nexus-lsp` already ships. Landing it under the contribution model from day one means we never have to migrate a fourth flat-TOML config later.

This pattern is consistent with the microkernel invariant ("new capability ⇒ IPC handler in service crate") and was a load-bearing simplification for the BL-076 and BL-081 first cuts. It has a real limitation, surfaced during the BL-081 review:

1. **No per-adapter UX customisation.** Each adapter has its own launch-config schema (codelldb's `cargo`-aware launch differs structurally from js-debug's `runtimeArgs`), its own variable formatters (Rust `Vec<T>` summary vs Python `__repr__`), and its own diagnostic conventions. The current TOML carries `name`, `command`, `args`, `file_types`, `env`, and an untyped `extra` JSON blob — there's nowhere to attach launch-config forms, variable renderers, or hover providers per adapter.
2. **No discoverability / marketplace path.** Users hand-edit TOML. There's no "install Rust debugger" UX, no version pinning, no signed-distribution story. Compare VS Code's per-debugger / per-language-server extension model — every debugger is an extension that contributes its own launch configurations and inline value renderers.
3. **Three nearly-identical patterns drift independently.** `nexus-lsp::config::LspServerSpec`, `nexus-mcp::McpServerSpec`, and `nexus-dap::config::DapAdapterSpec` are 80% the same shape with subtly different field names. Adding a new common feature (say, per-adapter resource limits) means editing three nearly-identical struct definitions, three parsers, three call sites.
4. **The host always boots.** `nexus-bootstrap` registers `com.nexus.lsp`, `com.nexus.mcp.host`, and (today, on the parked branch) `com.nexus.dap` whether the forge uses them or not. A forge with no `dap.toml` still pays the cost of `DapCorePlugin::on_init`. Cosmetic, but it conflicts with the "shell starts empty" stance from ADR 0011.

## Decision

**Proposed.** Lift adapter configuration from flat TOML to a plugin contribution point, shared across the three protocol hosts. The host crates remain core; each adapter becomes a plugin contribution (community-tier or first-party) that:

1. Declares the executable + launch shape via a manifest contribution.
2. May ship its own UI surface (launch-config form, variable renderer, hover provider).
3. Is install/uninstall/version-managed through the same mechanism as any other plugin.

### Shape (sketched, not final)

A community plugin contributes adapters via a new manifest section. The four protocol families share one contribution shape with per-family sub-tables:

```toml
# example: a "Rust debugging" plugin manifest fragment
id = "community.rust-debug"
name = "Rust Debugger (codelldb)"

[[contributes.protocolHosts.dap]]
id = "rust"
display_name = "Rust (codelldb)"
command = "codelldb"
args = ["--port", "0"]
file_types = ["rs"]
launch_config_schema = "./launch-config.schema.json"  # JSON Schema for the launch form
variable_renderers = ["rust_vec", "rust_option", "rust_hashmap"]  # references shell exports

[[contributes.protocolHosts.lsp]]
id = "rust-analyzer"
command = "rust-analyzer"
file_types = ["rs"]
root_markers = ["Cargo.toml"]
hover_renderer = "rust_hover"  # references a shell-side export

# Same shape for MCP:
[[contributes.protocolHosts.mcp]]
id = "rust-docs-mcp"
command = "rust-docs-mcp"
auto_connect = true

# And for ACP (Hermes Feature 7, not built yet — registering the shape now
# so the future `nexus-acp` crate inherits the contribution loader rather
# than ship a fourth flat-TOML form):
[[contributes.protocolHosts.acp]]
id = "hermes-rust-reviewer"
display_name = "Rust Code Reviewer (Hermes)"
command = "hermes-agent"
args = ["--profile", "rust-reviewer"]
# Optional declarative agent metadata — equivalent to LSP's `file_types` /
# DAP's `launch_config_schema`. Exact field set TBD during the Phase 4 spike.
capabilities = ["delegate", "tools", "memory"]
```

`nexus-lsp`, `nexus-dap`, `nexus-mcp`, and `nexus-acp` (when it's built) each gain a new `register_adapter` IPC verb that the plugin loader calls during contribution wiring; the runtime contribution set is merged with the legacy TOML so existing forges keep working through a transition window.

The launch-config / variable-renderer / hover-renderer keys reference shell-side exports the contributing plugin provides — so a community DAP plugin can ship a typed launch form (built against the schema) and a per-language variable-formatting hook, and the shell host (`nexus.debugger` / the editor) consumes them through the existing plugin export surface.

### Migration

1. **Phase 0 — ADR + spike.** Land this ADR, mint BL-113, prototype the contribution loader against `nexus-dap` (since it's on a branch already; touching adapter shape on a parked branch costs nothing).
2. **Phase 1 — DAP contribution model lands.** Bring BL-081 (the parked `bl-081-dap-debugger` branch) onto the new contribution shape, ship two example adapter plugins (`first-party.dap.rust`, `first-party.dap.node`). Keep `dap.toml` working as a legacy fallback.
3. **Phase 2 — LSP follows.** Refactor `nexus-lsp` to read contributions alongside `lsp.toml`. Migrate the bundled-server pattern to first-party plugins.
4. **Phase 3 — MCP follows.** Same for `nexus-mcp.host`. `mcp.toml` keeps working for user-authored entries; first-party / vetted MCP servers ship as contributions.
5. **Phase 4 — ACP greenfield.** When the Hermes-Feature-7 work (or whichever ACP integration BL ends up scoping) is picked up, the `nexus-acp` crate inherits the contribution model from day one. No legacy TOML to deprecate later because we never ship one.
6. **Phase 5 (optional) — retire TOML.** After two minor releases of overlap, mark the flat-TOML forms (LSP/DAP/MCP) deprecated and remove them. Decision deferred to that point.

### What stays

- Host crates (`nexus-lsp`, `nexus-dap`, `nexus-mcp`) remain core — they own the protocol, the connection pool, and the IPC surface. This is not a "make every adapter a separate Rust crate" proposal.
- Adapter executables stay external. A contribution doesn't bundle a debug adapter binary; it declares how to spawn one already on `$PATH` (with a path-discovery hint a future installer could feed off).
- The microkernel invariant holds: contributions flow through the existing plugin manifest path; no frontend gains a direct dependency on a service crate.

## Consequences

**Positive**
- Per-adapter UX (launch forms, value renderers, hover providers, agent capability descriptors) becomes possible without modifying any core crate.
- Marketplace + signing already work for plugins — adapter distribution rides on the existing capability path.
- One shared contribution loader serves four protocols, reducing the LSP/DAP/MCP/ACP-host code quadruplication.
- BL-081's "deferred launch-config picker" item collapses — the picker becomes schema-driven against contributions.
- The future `nexus-acp` crate inherits the contribution model on day one, so it never accretes a flat-TOML form that has to be migrated later.
- `nexus-bootstrap`'s ordering of LSP / DAP / MCP doesn't change; only how they discover their adapters does.

**Negative**
- New contribution point is non-trivial: requires a JSON Schema validator for launch-config forms, a shell-side export-resolution path for `variable_renderers` / `hover_renderer`, and a migration story for the three legacy TOMLs.
- Spike work — Phase 0 is ~1–2 days of design + scaffolding before any of the three hosts can move.
- Plugin-manifest growth: adding a sub-section per protocol host means more schema for community authors to learn. Mitigated by code-gen of typed builders.

**Risks**
- **Marketplace dependency.** First-party DAP/LSP adapters become plugins, which means the install story has to work for them. This pulls forward part of WI-44 (minimal marketplace).
- **Manifest churn.** Renaming `dap.toml`'s `adapter_type` to a `contributes.protocolHosts.dap.id` field is a one-way migration. Need overlap window + clear deprecation messaging.
- **Per-adapter version skew.** Today an operator pinning `codelldb 1.8` edits one TOML; under the new shape they swap a plugin version. That's strictly better with the marketplace but slightly worse without (need a plugin-pin mechanism).

## Open questions (resolved before Phase 0a)

1. **Hot-reload of contribution-defined adapters.** Resolved: plugin
   activate/deactivate is the lifecycle; the Phase 1 host-side wiring
   calls `register_adapter` from the plugin's activation closure and
   `unregister_adapter` from its deactivation closure. No additional
   wake-up loop in the host. The Phase 0a aggregator
   (`nexus-plugins::collect_contributions`) is pure, so the host calls
   it fresh on each lifecycle event.
2. **Where do `variable_renderers` and `hover_renderer` get executed?**
   Resolved: **shell-only**. The host crates stay protocol-only — they
   never resolve those identifiers. The shell looks them up in the
   contributing plugin's exports table when rendering. The Phase 0a
   types carry the identifier strings verbatim; no Rust-side
   resolution.
3. **Capability surface.** Resolved: **contribution wiring follows the
   command-contribution precedent — no new capability required at
   registration time.** A contributed adapter is treated as a
   declarative manifest entry, no different from a `[[ipc_command]]`
   contribution. The capabilities that *gate adapter usage at runtime*
   (e.g. spawning the executable, opening the TCP socket) ride on the
   plugin's existing capability grants (`process.spawn`, `net.connect`)
   per the standard capability path.
4. **Schema validation timing.** Resolved: **both sides validate,
   host-side is authoritative.** The shell renders the launch-config
   form against the JSON Schema referenced in
   `launch_config_schema` (richer error messages, pre-submit hints).
   The host crate re-validates on the `register_adapter` payload as the
   authoritative gate. Phase 0a does not yet ship validation — the
   Phase 1 DAP rollout is the first time the host sees one.

## Phase 0a — shipped 2026-05-14

The minimal foundational landing on `main`:

- New `ProtocolHostsContribution` + `LspProtocolHostReg` /
  `DapProtocolHostReg` / `McpProtocolHostReg` / `AcpProtocolHostReg`
  public types in `nexus-plugins::manifest`.
- New `[[registrations.protocol_hosts.{lsp,dap,mcp,acp}]]` TOML
  sections in `manifest.toml`, parsed and round-tripped through 7
  unit tests across the four families plus the empty-contribution case.
- New `nexus-plugins::contributions` module shipping
  `ContributedAdapter<T>`, `ContributedAdapterSet`, and the
  `collect_contributions(&[&PluginManifest])` aggregator that tags each
  contribution with the originating plugin id and returns the four
  family vectors. 4 unit tests cover the tagger + the all-empty
  fast-path.

The host crates do not yet consume the aggregator — that's Phase 1
(DAP first, on the parked `bl-081-dap-debugger` branch), Phase 2 (LSP),
Phase 3 (MCP), Phase 4 (ACP — greenfield from day one).

## References

- [BL-081 spec](../PRDs/BL-081-dap-debugger.md) — the parked branch that surfaced this question.
- ADR 0011 — plugin-first shell, "shell starts empty" stance.
- ADR 0023 / 0024 — recent precedent for unifying parallel mechanisms across `nexus-ai` and `nexus-agent`.
- VS Code's `contributes.debuggers` / `contributes.languages` schemas — the dominant mental model for per-protocol-adapter contributions.
