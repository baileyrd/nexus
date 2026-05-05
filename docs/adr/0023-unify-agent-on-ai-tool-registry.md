# ADR 0023: Unify Agent Planning on the AI Tool Registry

**Date:** 2026-05-05
**Status:** Proposed
**Audit reference:** [`docs/AI-INTERACTION-SURFACE-AUDIT.md`](../AI-INTERACTION-SURFACE-AUDIT.md) §2C / §5 (G7)

## Context

Today the AI plugin and the agent plugin run two parallel tool-call
mechanisms that don't share code:

- **AI tool-loop** (`crates/nexus-ai/src/core_plugin.rs::run_tool_dispatch_loop`):
  the model receives formal `ToolSchema`s from `ToolRegistry`, emits
  provider-native tool-use blocks (Anthropic `tool_use` / OpenAI
  `function_calls` / Ollama tool-calls), the loop executes them via
  the registry and feeds results back. Up to 8 rounds. Built-in tools
  + MCP bridge live here (post-G4/G5b).
- **Agent planner** (`crates/nexus-agent/src/llm.rs::LlmAgent`): the
  model receives `DEFAULT_SYSTEM_PROMPT` listing plugin ids in prose
  (`"com.nexus.storage, com.nexus.database, com.nexus.git, …"`) and
  is asked to emit a free-form JSON plan with `target_plugin_id` /
  `command_id` / `args` for each step. The agent extracts the JSON
  via `extract_json_object`, validates against `PlanDoc`, and the
  `PlanExecutor` dispatches each `ToolCall` over kernel IPC.

Two surfaces, two prompts, two failure modes. The duplication has
concrete costs:

1. **No tool schema discipline on the agent side.** The agent's prompt
   names plugins but doesn't describe argument shapes. The model
   guesses arg keys (`{"path": "..."}` vs `{"file_path": "..."}`),
   which surfaces as `IPC::CommandNotFound` or `InvalidInput` only at
   execution time. The AI side gets formal JSON Schema enforcement
   from the provider's tool-use validator before the call reaches the
   wire.
2. **No native MCP tooling in the agent.** G5b bridged MCP tools into
   the AI registry as `mcp__<server>__<tool>`. The agent's planner
   doesn't know any of those exist — its prompt lists `com.nexus.*`
   ids only. To use an MCP tool from the agent, the user has to spell
   the IPC call manually as `(com.nexus.mcp.host, call_tool, {...})`,
   which the model rarely gets right without examples.
3. **Brittle JSON extraction.** `extract_json_object` cuts the first
   `{…}` block out of the model's response. Models that wrap their
   answer in prose, helpful framing, or unexpected tool descriptions
   trip the extractor. Provider tool-use blocks are structured and
   don't have this failure mode.
4. **Drift risk.** Adding a new tool to the AI registry (G4 added
   three) doesn't propagate to the agent. The agent's prompt has to
   be edited separately to mention them, with no compile-time guard
   against forgetting.

The agent has its own user-visible affordances (per-step approval
policy, plan history persistence, archetypes with custom prompts) that
the AI tool-loop doesn't provide. Those are real and worth keeping —
they're not the duplication problem.

## Decision

Migrate the agent to the AI tool registry's tool-call mechanism in two
phases. **Phase 1** swaps the wire format and the tool schema source
without changing the agent's plan-then-execute data model. **Phase 2**
(separate ADR) considers collapsing plan/execute into a single
tool-loop session.

### Phase 1 — this ADR

**Wire format.** `LlmAgent::plan` calls the provider via
`AiProvider::chat_turn_with_tools` (the same entry point
`run_tool_dispatch_loop` uses) instead of `ChatDriver::chat`. The
provider returns a `ChatTurnOutput { text, tool_calls }`; each
`ToolCall` from the provider becomes one `Step`:

```text
Step {
    id: "step-{N}",
    description: tool_call_description_from_schema_or_text_block,
    tool_call: Some(ToolCall {
        target_plugin_id: derived_from_tool_name,
        command_id: derived_from_tool_name,
        args: tool_call.input,
    }),
}
```

Provider-native tool-use carries an opaque tool *name*; the agent
needs `target_plugin_id` + `command_id` to dispatch. We adopt the
same convention `mcp_bridge` uses for MCP tools and extend it to
built-ins:

| Tool name in registry | Maps to |
|---|---|
| `read_file` | `("com.nexus.storage", "read_file")` |
| `write_file` | `("com.nexus.storage", "write_file")` |
| `search_forge` | `("com.nexus.storage", "search")` |
| `list_backlinks` | `("com.nexus.storage", "backlinks")` |
| `git_log` | `("com.nexus.git", "log")` |
| `mcp__<server>__<tool>` | `("com.nexus.mcp.host", "call_tool")` w/ `{server, tool, …}` args |

The mapping lives next to the registry — a new
`crates/nexus-ai/src/tools/dispatch_target.rs` exposing
`fn dispatch_target(tool_name: &str) -> (String, String,
serde_json::Value)`. The bootstrap-side `ChatDriver` wrapper hands
the agent the same `Arc<ToolRegistry>` the AI plugin uses, plus this
mapping function.

**Schemas.** `DEFAULT_SYSTEM_PROMPT` drops the prose plugin list. The
provider receives the registry's `ToolSchema`s, same as `stream_chat`.
What stays in the system prompt: the agent's posture (planner role,
"prefer fewer larger steps", "omit tool calls for informational
updates"). Archetype prompts (`WRITER_SYSTEM_PROMPT`, …) keep their
domain framing but lose the JSON-shape boilerplate.

**Plan/Step/ToolCall data model.** Unchanged. The executor, approval
policy, and persistence story all keep working. From the user's
perspective the agent still produces a Plan they can review; the only
difference is the Plan is built from provider tool-use blocks instead
of from JSON-extracted text.

**`tools` policy.** The agent's `ChatDriver` passes
`AiToolPolicy::Auto` by default and `AutoWithMcp` when the caller
opts in. Phase 1 doesn't add a new policy variant.

**Backward compatibility.** Existing plan-history JSON on disk
deserialises into the same `Plan` shape. No on-disk migration. The
`PlanDoc` / `StepDoc` types and `extract_json_object` get deleted.

### Phase 2 — deferred follow-up ADR

The agent's plan-then-execute split exists for two reasons that don't
hold under tool-loop semantics:

- **Pre-execution approval.** Today the user sees the whole plan
  before any step runs. Under a tool-loop session, approval would
  happen per tool call — closer to "will you let me read X?" than
  "here's the plan, approve it as a whole".
- **Persistence.** Plans are stored as completed-or-aborted artifacts.
  A tool-loop session would persist as a conversation transcript with
  embedded tool calls.

A clean Phase 2 ADR would have to settle the approval UX and the
persistence model first. Out of scope here.

## Alternatives considered

1. **Status quo, two mechanisms.** Keeps duplication and the four
   costs above. Rejected; the audit flagged the gap explicitly.
2. **Have the agent register its own tool registry.** The audit's
   wording suggested this. Doesn't solve the wire-format brittleness
   or MCP coverage; just moves the schema duplication. Rejected.
3. **Phase-1-and-2 in one drop.** Triples the change surface. The
   data-model collapse needs UX answers (approval, persistence) the
   agent system hasn't settled. Rejected for risk.
4. **Reverse the dependency — make AI use the agent's planner under
   the hood.** Conflicts with the AI plugin's streaming UX (chat is
   token-by-token, agent planning is single-turn-then-plan).
   Rejected.

## Consequences

### Wins
- Schema validation runs at the provider layer, before IPC.
- MCP tools (G5b) become reachable from the agent for free when the
  caller opts into `AutoWithMcp`.
- One place to add a new tool: `register_*_builtins` in
  `crates/nexus-ai/src/tools/functions.rs`.
- Removes the JSON-extraction failure mode entirely.

### Costs / disruption
- `LlmAgent::new(driver)` constructor changes shape: the production
  driver now needs an `Arc<ToolRegistry>` plus the dispatch_target
  mapping. Test drivers (`CannedDriver` in `llm.rs:212`) need to
  update.
- `PlanDoc` / `StepDoc` deleted. Any out-of-tree consumer that
  imported them breaks. None known.
- `DEFAULT_SYSTEM_PROMPT` and the three archetype prompts get
  rewritten. Behaviour drift in plan output is expected and desired.
- Plan history written under the old planner is still loadable
  (same `Plan` shape on disk), but plans produced before the change
  may carry tool calls the new dispatcher rejects (e.g. an agent that
  emitted `target_plugin_id="com.nexus.terminal", command_id="exec"`
  even though no such command exists). This wasn't reliable before
  either; the old executor would just have failed at runtime.
- `dispatch_target.rs` becomes a load-bearing file: every new
  built-in tool added to the AI registry needs an entry here, or the
  agent can't call it. The mapping function `panic!`s on unknown
  names in tests so the mismatch is loud.
- Phase 1 doesn't yet bring server-enforced `tools` policy (that's
  ADR 0022 Phase 2). An agent with `ai.chat` can still ask for
  `AutoWithMcp` and get MCP access regardless of intent — same as
  any other `stream_chat` caller today.

### Migration sequencing
- Land `dispatch_target` first as a no-op addition to `nexus-ai`.
- Switch `LlmAgent` over.
- Delete `PlanDoc` / `StepDoc` / `extract_json_object`.
- Update three archetype prompts in one commit.
- One PR; existing tests prove the executor + approval flow still
  works against synthetic plans.

## Open follow-ups

- **Phase 2 ADR** on plan-vs-tool-loop semantics: approval UX,
  persistence model, archetype prompt styles.
- The agent's `delegate` / `parallel` / `pipeline` orchestrator
  surfaces (BL-027) currently produce composed plans; Phase 1 leaves
  their internals alone but they should be re-examined when Phase 2
  lands.
- `dispatch_target` is a candidate for being driven by registry
  metadata (each `ToolSchema` carrying its `(target_plugin_id,
  command_id)`) rather than a hand-written match. Cleanup, not a
  blocker.
