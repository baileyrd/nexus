# Amplifier Plugin Laundering (#189 / R6)

> **Status:** Formally accepted residual risk. Mitigations enumerated below.
> **As of:** 2026-06-01. Tracking: [#189](https://github.com/baileyrd/nexus/issues/189).
> **Predecessor:** Issue #77 (closed 2026-05-01) — per-handler cap-matrix + observability for terminal / MCP spawn handlers.

## Threat model

The kernel's IPC dispatch performs an unconditional `Capability::IpcCall` check
on every `context.ipc_call(...)` (`crates/nexus-kernel/src/context_impl.rs`).
A plugin that holds `IpcCall` can therefore reach *any* IPC handler
the matrix doesn't gate further. Amplifier plugins — `nexus-agent`,
`nexus-workflow`, and to a lesser extent `nexus-ai-runtime` — exist to
execute user-authored plans / workflows / scheduled tasks that call into
other plugins on the user's behalf. If an amplifier holds nothing more
than `IpcCall`, anything an LLM-generated plan or an attacker-influenced
workflow file can ask it to do still flows through `IpcCall`.

The audit's framing: *"amplifier plugins holding `IpcCall` can launder
calls into high-impact handlers the matrix doesn't gate further."*

## Existing mitigations

The residual surface is narrower than the framing suggests because four
in-tree mitigations already attenuate it.

### 1. Per-context scoped capability sets

Bootstrap hands each amplifier plugin a `KernelPluginContext` with a
deliberately minimal `CapabilitySet`, not `Capability::ALL`. Source:
`crates/nexus-bootstrap/src/lib.rs`:

| Amplifier | Capabilities granted | High-impact caps deliberately withheld |
|---|---|---|
| `nexus-agent` | `IpcCall`, `FsRead`, `FsWrite`, `AiChat`, `AiToolsWrite`, `AiRuntimeSubmit`, `AiRuntimeObserve` | `ProcessSpawn`, `NetHttp`, `FsReadExternal`, `FsWriteExternal`, `AiConfigWrite`, `AiActivityWrite`, `AiToolsMcp` |
| `nexus-workflow` | `IpcCall`, `AiChat`, `AiRuntimeSubmit` | All others, especially `ProcessSpawn`, `NetHttp`, `AiConfigWrite`, `AiActivityWrite`, `AiRuntimeObserve` (BL-134 Phase 3: observe is checked at the follow-up `wait_for` step) |
| `nexus-ai-runtime` | `IpcCall`, `AiChat`, `EventsPublish` | All others |
| `nexus-audio` | `IpcCall`, `NetHttp` | `ProcessSpawn`, `AudioRecord` (caller-facing gate, not self) |

The `FsRead` / `FsWrite` grants for `nexus-agent` are confined to the
forge root by the kernel's `confine_path` (`context_impl.rs`); they do
not grant external-filesystem access.

The withheld caps mean: even though the amplifier holds `IpcCall`, calls
into handlers that *require* `ProcessSpawn` (terminal sessions, MCP /
DAP / ACP server spawn) reject the amplifier directly — they hit the
per-handler cap check before reaching the handler body.

### 2. Per-handler `cap_matrix.toml`

`crates/nexus-bootstrap/cap_matrix.toml` classifies every IPC handler
the system registers (336 rows as of 2026-06-01). Each row is either:

- `caps = [...]` — caller must hold every listed cap on top of `IpcCall`.
  66 handlers are cap-gated this way.
- `unrestricted = "<rationale>"` — handler is intentionally reachable
  by any caller with `IpcCall`. 270 handlers, each carrying a one-line
  rationale.

The cap-gated set covers the audit's named high-impact surface:

| Cap | Gated handlers (representative) |
|---|---|
| `process.spawn` | `com.nexus.terminal::{create_session, repl_start, send_input, send_raw_input, run_saved, adhoc_promote, repl_eval}`, `com.nexus.mcp.host::connect`, `com.nexus.dap::{launch, attach}`, `com.nexus.acp::initialize` |
| `net.http` | `com.nexus.ai` providers, `com.nexus.linkpreview`, `com.nexus.notifications` webhook / SMTP, `com.nexus.collab` relay client, `com.nexus.audio` provider, `com.nexus.git::{push, push_tags}` |
| `fs.write.external` / `fs.read.external` | `com.nexus.storage` paths that resolve outside the forge root |
| `ai.config.write` | `com.nexus.ai::set_config` (hot-swap provider credentials) |
| `security.write` / `security.audit.write` | Keyring writes + audit-log truncation |
| `protocol.host.contribute` | DAP / LSP / MCP-host / ACP `register_*` / `unregister_*` lifecycle verbs |

Cross-reference: see [`capabilities.md`](capabilities.md) for the full
33-entry capability inventory and risk classification.

### 3. Completeness gate — `cap_matrix_complete`

`crates/nexus-bootstrap/tests/cap_matrix_complete.rs` boots a full
runtime, enumerates every `(plugin, command)` pair in the live IPC
registry, and fails if any handler lacks a classification. The intended
failure mode is *"you added a new handler without classifying it in
`cap_matrix.toml`."* This prevents unclassified handlers from silently
landing on the `IpcCall`-only default — a new handler's classification
is a required code-review decision.

### 4. Audit-tagged observability

`crates/nexus-workflow/src/handlers/run.rs` and the digest scheduler
emit `audit = true` `tracing::warn!` lines listing the implied caller
caps on every invocation. The audit `<forge>/.forge/.kernel/audit.db`
(via `nexus-security`) persists every `CapabilityDenied` event with
90-day retention, queryable from operator tools through
`com.nexus.security::query_audit_log`. An amplifier-launder attempt
that hits a cap-gated handler is therefore both denied AND visible.

## Residual risk

After the mitigations above, the remaining surface is:

**An amplifier plugin can reach handlers classified `unrestricted` in
`cap_matrix.toml`** — 270 of 336 entries (≈ 80%).

The 270 `unrestricted` handlers are scoped to forge-local reads,
metadata probes, status queries, and explicit-grant write paths
(e.g., `com.nexus.storage::write_file`, which is gated *inside* the
handler by `confine_path`, not at the matrix). Each row's
`unrestricted = "<why>"` rationale records the design call. They are
not laundering vectors in the sense the audit names because the
high-impact verbs are already gated by point 2 above.

The genuine residual is the **transitive surface**: a future handler
added with `unrestricted = "harmless probe"` might compose with another
handler in a way that promotes the combined effect to "high impact." The
cap-matrix entry is per-handler and cannot reason about handler chains.
This is the same shape as the "amplifier holding `IpcCall` reaches every
unrestricted handler" framing, just re-stated at the entry level.

## Formal acceptance

We accept the residual risk under these conditions:

1. **Per-handler classification stays mandatory.** Every new handler
   lands with either `caps = [...]` or `unrestricted = "<why>"`. The
   `cap_matrix_complete` test enforces this; removing it requires a
   replacement enforcement mechanism.
2. **Amplifier capability sets stay minimal.** The
   `agent_capabilities()` / `workflow_capabilities()` /
   `ai_runtime_capabilities()` / `audio_capabilities()` functions in
   `crates/nexus-bootstrap/src/lib.rs` carry the threat-model rationale
   in their docstring. Broadening any of them requires an explicit
   audit-trail comment and the corresponding `# AUDIT:` entry in
   `cap_matrix.toml` when the broadening promotes a previously gated
   handler to reachable.
3. **High-impact verbs stay cap-gated.** Any new handler that spawns
   processes, performs external I/O, mutates credentials, truncates
   audit logs, binds listeners, or rotates provider config requires a
   `caps = [...]` classification, never `unrestricted`. The
   `[`risk classification`]`(capabilities.md#risk-classification)
   section of `capabilities.md` provides the test: "is the action
   visible to or observable by other peers / persistent / irreversible /
   capable of disclosing credentials?" If yes, gate it.
4. **Audit trail covers laundering attempts.** `CapabilityDenied`
   events for amplifier-originated calls remain queryable via
   `com.nexus.security::query_audit_log` so operators can detect
   prolonged probe patterns.

## Path to a stricter posture (out of scope here)

If kernel-side enforcement of the transitive surface becomes a future
requirement, the design space includes:

1. **Trust-level pivot on `unrestricted`.** Introduce a third
   classification — e.g. `unrestricted_to_core = "<why>"` — that
   community-tier plugins cannot reach even with `IpcCall`. Existing
   `unrestricted` rows are reclassified per handler.
2. **Per-call propagation of caller's effective cap set.** The kernel
   already tracks the caller's `CapabilitySet` for the matrix check.
   Extending that to transitively check handlers the call *would*
   reach is a larger architectural change (would need a way to declare
   "this handler internally calls X with these caps").
3. **WASM-tier separation for amplifiers.** Run user-authored
   workflows and agent plans through the WASM sandbox (`nexus-plugins`
   sandbox) instead of as in-process amplifiers; the sandbox already
   applies per-handler cap checks and the `internal = true` gate
   (PR #206, #230). The amplifier itself stays first-party but the
   user-authored execution surface becomes a community-tier guest.

None of these is currently planned; all three are documented here so a
future review can pick one without re-litigating the threat model.

## See also

- [`capabilities.md`](capabilities.md) — full capability vocabulary +
  risk classification.
- [`reference/audit-flags.md`](reference/audit-flags.md) — live
  `# AUDIT:` flags in `cap_matrix.toml`.
- [`crates/nexus-bootstrap/cap_matrix.toml`](../../crates/nexus-bootstrap/cap_matrix.toml)
  — per-handler classification source of truth.
- [`crates/nexus-bootstrap/src/lib.rs`](../../crates/nexus-bootstrap/src/lib.rs)
  — `agent_capabilities` / `workflow_capabilities` /
  `ai_runtime_capabilities` / `audio_capabilities` per-context scopes.
- [`crates/nexus-bootstrap/tests/cap_matrix_complete.rs`](../../crates/nexus-bootstrap/tests/cap_matrix_complete.rs)
  — completeness gate.
- ADR 0002 (hierarchical capability strings) — root design.
- Issue [#77](https://github.com/baileyrd/nexus/issues/77) — closed
  predecessor; landed the per-handler cap-matrix and observability for
  terminal / MCP spawn handlers.
