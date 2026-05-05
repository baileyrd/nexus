# ADR 0022: Per-Handler Capabilities for the AI Plugin

**Date:** 2026-05-05
**Status:** Accepted
**Audit reference:** [`docs/AI-INTERACTION-SURFACE-AUDIT.md`](../AI-INTERACTION-SURFACE-AUDIT.md) §4 (G6)

## Context

The AI Interaction Surface Audit flagged a gap in capability granularity for
`com.nexus.ai`:

> No `ai.chat` capability exists. Any caller with `ipc.call` can invoke any
> AI handler — no per-handler granularity (e.g. allow `chat`, deny
> `index_file`). The `tools` request argument is client-controlled, not
> server-enforced.

`com.nexus.ai` exposes 19 IPC handlers spanning very different blast
radiuses:

- **Read-only chat surface** (`stream_chat`, `stream_ask`, `ask`,
  `status`, `config`) — model produces text; tool side-effects gated by
  the registry's own capabilities.
- **Indexing** (`index_file`, `index_trigger`, `index_status`) —
  reads + embeds files, writes vectorstore rows. Cheap individually
  but a tight loop can exhaust an embedding API quota or fill the
  index.
- **Session storage** (`session_load/save/list/delete`) — writes
  `<forge>/.forge/chat/sessions/`, can be used to read or destroy
  another caller's chat history.
- **Configuration** (`set_config`) — hot-swaps provider credentials at
  runtime. Plain `IpcCall` is enough today to rotate the user's API
  key out from under them.
- **Activity log** (`activity_list/clear`) — `clear` truncates BL-037's
  audit trail; equivalent to wiping a tamper-evident log.
- **Frontmatter enrichment** (`enrich_file/apply`) — model-produced
  edits applied to a file.

Today every one of these is reachable by any caller holding `ipc.call`.
Workflow contexts and the agent's planner explicitly drop most caps
([#73]) but keep `ipc.call` to call into AI — meaning a malicious
workflow step or agent tool call can rotate API credentials, wipe the
activity log, or destroy a chat session.

The kernel already has the infrastructure to fix this. Issue [#77]
landed `IpcDispatcher::required_caller_caps(target, command)` and
`SharedPluginLoader::add_cap_requirement(target, command, caps)`, used
today to gate `terminal::create_session` and `mcp.host::connect` behind
`ProcessSpawn`. Adding `ai.*` capabilities is a straight extension of
that mechanism — no new kernel machinery needed.

[#73]: https://github.com/nexus/nexus/issues/73
[#77]: https://github.com/nexus/nexus/issues/77

## Decision

Introduce six per-handler capabilities under the `ai.*` namespace
(reserved by [ADR 0002]) and wire them through
`add_cap_requirement` in `nexus-bootstrap`.

[ADR 0002]: 0002-hierarchical-capability-strings.md

### Capability inventory

| String | Variant | Risk | Gates |
|---|---|---|---|
| `ai.chat` | `AiChat` | Medium | `stream_chat`, `stream_ask`, `ask`, `semantic_search`, `enrich_file` |
| `ai.index` | `AiIndex` | Low | `index_file`, `index_trigger` |
| `ai.session.read` | `AiSessionRead` | Low | `session_load`, `session_list` |
| `ai.session.write` | `AiSessionWrite` | Low | `session_save`, `session_delete` |
| `ai.config.write` | `AiConfigWrite` | **High** | `set_config` |
| `ai.activity.write` | `AiActivityWrite` | Medium | `activity_clear` |

Handlers not in the table (`status`, `config`, `index_status`,
`vectorstore_count`, `activity_list`, `apply`) are read-only or already
gated by an existing capability when they perform side effects (e.g.
`apply` writes a file via the storage IPC, which requires `fs.write` on
storage's side via the storage handler's own checks). They keep the
`ipc.call`-only default.

`ai.config.write` is **High** by analogy with `process.spawn`: it
mutates a credential surface. Users will be prompted to grant it
explicitly, same as today's HIGH-risk caps.

`ai.chat` is **Medium**, not Low: `stream_chat` invokes tools which can
mutate the forge. It's not High because the model's tool calls are
themselves gated by storage-side `fs.write` etc., and tools that need
elevated capabilities (e.g. a future `terminal_exec`) will demand
`process.spawn` on the caller through the same per-handler mechanism.

### Server-side `tools` policy enforcement

Today the `tools` field on `AiStreamChatArgs` (`AiToolPolicy::Auto`,
`None`, `AutoWithMcp`) is honoured purely by the AI plugin's own logic.
A malicious caller could pass `Auto` even when policy says they should
get `None`. The audit flagged this as "client-controlled, not
server-enforced".

This ADR does **not** change the wire shape. Instead it relies on the
caller cap to scope what the field can mean:

- A caller without `ai.chat` cannot reach `stream_chat` at all —
  `tools` is moot.
- A caller with `ai.chat` but without (a future) `ai.tools.write` is
  given only the read-only built-ins (`read_file`, `search_forge`,
  `list_backlinks`, `git_log`) regardless of what `tools` requested.
  Write tools (`write_file`) and `AutoWithMcp` require an additional
  capability to be added in a follow-up ADR — out of scope here so the
  immediate landing is small.

Today's behaviour (everything-or-nothing) is preserved during the
migration. The follow-up ADR proposes `ai.tools.write` as the gate.

### Rollout

- **Phase 1 (this ADR):** add the six caps, wire `add_cap_requirement`
  in bootstrap. Update default cap sets for the four contexts that
  legitimately use AI today: CLI invoker, TUI invoker (no AI yet), MCP
  host (none), shell (broad). Workflow + agent contexts get `ai.chat`
  and `ai.session.read` only — no `ai.config.write`, no
  `ai.activity.write`.
- **Phase 2 (follow-up):** introduce `ai.tools.write` and `ai.tools.mcp`
  to make the `tools` policy server-enforced.

## Alternatives considered

1. **Single `ai` capability.** Trivial to implement but defeats the
   point — workflow contexts would still be able to wipe the activity
   log. Rejected.
2. **Capability per handler (19 caps).** Maximally granular, but most
   handlers don't have meaningfully different blast radiuses. Hard for
   users to reason about a 19-row consent prompt. Rejected.
3. **Move the gate inside the AI plugin** (handler-side checks instead
   of `add_cap_requirement`). Would require every `dispatch` arm to
   re-implement the capability check the kernel already does centrally;
   loses the CapabilityDenied audit log. Rejected.
4. **Wrap `tools` enforcement into `ai.chat` itself** (e.g. `ai.chat`
   implies write tools, `ai.chat.read` doesn't). Conflates two axes —
   write tools and MCP are independent of "can you talk to the model"
   and want their own knobs. Rejected; deferred to Phase 2.

## Consequences

- Adding six `Capability` variants is an IPC drift event: regenerate
  TS bindings + JSON schemas via `scripts/check_ipc_drift.sh`.
- Plugin manifests that reference the new caps gain new strings in
  `Capability::as_str()`'s match — type-safe per ADR 0002.
- Workflow + agent default cap sets shrink. Any in-tree workflow that
  was implicitly relying on `set_config` access will break — there are
  no such workflows today, and breaking them is the whole point.
- High-risk `ai.config.write` triggers the same persisted-grant prompt
  as `process.spawn` per ADR 0002. First-run UX: shell prompts once.
- The `tools` policy field stays honour-system in Phase 1. The ADR
  makes that explicit so reviewers don't read the AI capability work as
  closing the audit's full §4 finding.
- Failure mode if a caller drops a needed cap: kernel returns
  `CapabilityDenied` before the handler runs, with an audit log line.
  Same UX as the existing `process.spawn` gate.

## Open follow-ups

- Phase 2 ADR: `ai.tools.write` + `ai.tools.mcp`. Closes the
  "client-controlled tools policy" half of the audit finding.
- Manifest schema docs (`docs/writing-your-first-plugin.md`,
  `shell/docs/writing-a-plugin.md`) need an `ai.*` section.
- Audit doc §4 ("Capability Gating — Weak") needs an amendment once
  Phase 1 lands.
