# ADR 0027: Protocol-Host Contribution Model for LSP / DAP / MCP

**Date:** 2026-05-13
**Status:** Proposed — open for review. Tracks as **BL-113** in the backlog.
**Related:** [ADR 0011](0011-active-shell-target.md) (plugin-first shell), [BL-076](../PRDs/BACKLOG_COMPLETED.md) (nexus-lsp host), [BL-081](../PRDs/BL-081-dap-debugger.md) (nexus-dap host, parked on `bl-081-dap-debugger` pending this ADR).

## Context

Three Nexus core plugins host external-process protocol adapters today:

| Crate | Host plugin | Adapter config | Examples |
|---|---|---|---|
| `nexus-lsp` | `com.nexus.lsp` | `<forge>/.forge/lsp.toml` | rust-analyzer, typescript-language-server |
| `nexus-mcp` | `com.nexus.mcp.host` | `<forge>/.forge/mcp.toml` | filesystem, git, custom MCP servers |
| `nexus-dap` | `com.nexus.dap` | `<forge>/.forge/dap.toml` | codelldb, debugpy, js-debug, dlv |

All three follow the same shape: the *host* is a core Rust plugin registered at bootstrap; the *adapters* are external executables named in a flat TOML config. The host proxies a protocol surface (JSON-RPC for LSP/MCP, a `type`-tagged JSON envelope for DAP) over IPC and republishes server-pushed messages on the kernel bus.

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

A community plugin contributes adapters via a new manifest section:

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
```

`nexus-lsp`, `nexus-dap`, and `nexus-mcp` each gain a new `register_adapter` IPC verb that the plugin loader calls during contribution wiring; the runtime contribution set is merged with the legacy TOML so existing forges keep working through a transition window.

The launch-config / variable-renderer / hover-renderer keys reference shell-side exports the contributing plugin provides — so a community DAP plugin can ship a typed launch form (built against the schema) and a per-language variable-formatting hook, and the shell host (`nexus.debugger` / the editor) consumes them through the existing plugin export surface.

### Migration

1. **Phase 0 — ADR + spike.** Land this ADR, mint BL-113, prototype the contribution loader against `nexus-dap` (since it's on a branch already; touching adapter shape on a parked branch costs nothing).
2. **Phase 1 — DAP contribution model lands.** Bring BL-081 (the parked `bl-081-dap-debugger` branch) onto the new contribution shape, ship two example adapter plugins (`first-party.dap.rust`, `first-party.dap.node`). Keep `dap.toml` working as a legacy fallback.
3. **Phase 2 — LSP follows.** Refactor `nexus-lsp` to read contributions alongside `lsp.toml`. Migrate the bundled-server pattern to first-party plugins.
4. **Phase 3 — MCP follows.** Same for `nexus-mcp.host`. `mcp.toml` keeps working for user-authored entries; first-party / vetted MCP servers ship as contributions.
5. **Phase 4 (optional) — retire TOML.** After two minor releases of overlap, mark the flat-TOML form deprecated and remove it. Decision deferred to that point.

### What stays

- Host crates (`nexus-lsp`, `nexus-dap`, `nexus-mcp`) remain core — they own the protocol, the connection pool, and the IPC surface. This is not a "make every adapter a separate Rust crate" proposal.
- Adapter executables stay external. A contribution doesn't bundle a debug adapter binary; it declares how to spawn one already on `$PATH` (with a path-discovery hint a future installer could feed off).
- The microkernel invariant holds: contributions flow through the existing plugin manifest path; no frontend gains a direct dependency on a service crate.

## Consequences

**Positive**
- Per-adapter UX (launch forms, value renderers, hover providers) becomes possible without modifying any core crate.
- Marketplace + signing already work for plugins — adapter distribution rides on the existing capability path.
- One shared contribution loader serves three protocols, reducing the LSP/DAP/MCP-host code triplication.
- BL-081's "deferred launch-config picker" item collapses — the picker becomes schema-driven against contributions.
- `nexus-bootstrap`'s ordering of LSP / DAP / MCP doesn't change; only how they discover their adapters does.

**Negative**
- New contribution point is non-trivial: requires a JSON Schema validator for launch-config forms, a shell-side export-resolution path for `variable_renderers` / `hover_renderer`, and a migration story for the three legacy TOMLs.
- Spike work — Phase 0 is ~1–2 days of design + scaffolding before any of the three hosts can move.
- Plugin-manifest growth: adding a sub-section per protocol host means more schema for community authors to learn. Mitigated by code-gen of typed builders.

**Risks**
- **Marketplace dependency.** First-party DAP/LSP adapters become plugins, which means the install story has to work for them. This pulls forward part of WI-44 (minimal marketplace).
- **Manifest churn.** Renaming `dap.toml`'s `adapter_type` to a `contributes.protocolHosts.dap.id` field is a one-way migration. Need overlap window + clear deprecation messaging.
- **Per-adapter version skew.** Today an operator pinning `codelldb 1.8` edits one TOML; under the new shape they swap a plugin version. That's strictly better with the marketplace but slightly worse without (need a plugin-pin mechanism).

## Open questions

1. **Hot-reload of contribution-defined adapters.** Today `dap.toml` is read at `on_init`; a contribution change after boot needs a re-scan. Existing plugin lifecycle (`activate` / `deactivate`) already covers this — verify it round-trips a `register_adapter` / `unregister_adapter` IPC pair.
2. **Where do `variable_renderers` and `hover_renderer` get executed?** The shell-side export path is the natural answer; the question is whether the host crate sees them at all or whether the shell consumes them out-of-band against the same plugin id. Leaning toward "shell-only" so the Rust host crate stays protocol-only.
3. **Capability surface.** A community plugin registering itself as `com.nexus.dap`'s adapter is implicitly granting itself a slice of the debugger's capability footprint. Need to decide whether `dap.register_adapter` is a new capability or whether contribution wiring bypasses the consent path (precedent: command contributions don't require a capability today).
4. **Schema validation timing.** Validate launch-config submission against the JSON Schema in the shell, in the host crate, or both? Shell-side gives a richer error; host-side is the authoritative gate. Likely both, with the shell-side validation surfacing pre-submit hints.

## References

- [BL-081 spec](../PRDs/BL-081-dap-debugger.md) — the parked branch that surfaced this question.
- ADR 0011 — plugin-first shell, "shell starts empty" stance.
- ADR 0023 / 0024 — recent precedent for unifying parallel mechanisms across `nexus-ai` and `nexus-agent`.
- VS Code's `contributes.debuggers` / `contributes.languages` schemas — the dominant mental model for per-protocol-adapter contributions.
