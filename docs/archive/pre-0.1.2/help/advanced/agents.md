# Agents (tool-using AI loops)

An **agent** is an AI loop that can call tools, observe their results,
and decide what to do next. Nexus ships archetypes for common
workflows — Writer, Coder, Researcher — and exposes the same engine
for custom agents.

## Built-in archetypes

| Archetype | What it does |
|---|---|
| **Writer** | Drafts and revises notes; tools: `read_note`, `update_note`, `search`, `backlinks` |
| **Coder** | Edits code with diff review; tools: `read_file`, `write_file`, `run_command` (gated) |
| **Researcher** | Multi-step Q&A across the forge; tools: `search`, `read_note`, `outgoing_links`, `web_fetch` (if configured) |

## Run an agent

From the shell, **AI Chat → Agents → Run agent…**. Pick an archetype,
describe the task, click **Start**.

CLI:

```bash
nexus agent run \
  "Summarize what I've written about wikilinks across all notes." \
  --archetype researcher

# Produce a plan without executing it
nexus agent plan \
  "Summarize what I've written about wikilinks across all notes." \
  --archetype researcher
```

The CLI surface is intentionally narrow — `plan` and `run`. Listing
prior runs and browsing history are shell-only today (AI Chat →
Agents → Sessions); the on-disk transcripts at
`<forge>/.forge/agents/<session-id>.json` are the source of truth
and you can grep / cat them directly.

## Stepwise approval

By default, every tool call is shown to you before it runs:

> Agent wants to: `update_note(path="launch.md", content=...)`
>
> [Approve] [Approve all this run] [Edit] [Reject]

You can toggle approval modes per agent:

- **Manual** — approve every tool call
- **Auto-read** — auto-approve read-only tools, prompt for writes
- **Auto** — full autopilot (use with care; logs every action)

Set the default in `.forge/ai.toml`:

```toml
[agent]
approval_mode = "auto-read"
```

## Tools

Agents call tools through capability-gated IPC. The toolset for an
agent is configurable; archetypes have sensible defaults. You can also
expose any plugin's IPC handler as a tool by adding it to the agent's
manifest.

External MCP servers registered in `.forge/mcp.toml` are
**auto-discovered** as tools — anything Claude Code can do, your
local Nexus agent can do too.

## History

Every run is persisted at `<forge>/.forge/agents/<session-id>.json`
with:

- The original task
- Every model turn (prompt + completion)
- Every tool call (args + result + duration)
- Approvals and rejections

You can replay a run, fork from any step, or export the session as
markdown for archival.

## Cost and limits

Agent loops can be expensive — multi-step plans hit the model
multiple times. Set a per-run cap:

```toml
[agent]
max_steps = 20
max_tokens_per_run = 50000
```

When a cap is hit the agent halts cleanly with a summary of what it
did.

## When to use an agent (not chat)

- The task needs multiple sequential decisions ("read X, then update Y
  based on what's in X").
- The task needs tool use (search, file write, command execution).
- The task is repeatable — once you have an agent that works, you can
  re-run it with new inputs.

For one-shot Q&A, [chat](../ai/chat.md) is faster and cheaper.

## Custom agents

Plugins contribute custom archetypes through the extension API:

```ts
context.agents.register({
  id: 'my-archetype',
  systemPrompt: '...',
  tools: ['com.nexus.storage:read_note', 'com.nexus.storage:search'],
});
```

See `docs/shell/plugin-api.md`.
