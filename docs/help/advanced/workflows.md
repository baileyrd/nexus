# Workflows (automation)

A **workflow** is a multi-step automation: a trigger plus a series of
actions. Triggers can be cron schedules or manual invocations; actions
can be IPC calls, shell commands, AI prompts, or custom plugin steps.

## File format

Workflows live as `.workflow.toml` files anywhere in the forge or
under `<forge>/.forge/workflows/`:

```toml
name = "Daily journal"
description = "Create today's daily note from a skill template."
trigger = { type = "cron", cron = "0 8 * * *" }

[[steps]]
id = "render"
type = "skill"
skill = "daily-journal"
args = { date = "{{today}}" }

[[steps]]
id = "create"
type = "ipc"
plugin = "com.nexus.storage"
command = "create_note"
args = { path = "daily/{{today}}.md", content = "{{steps.render.output}}" }
```

## Triggers

| Type | Example | Status |
|---|---|---|
| `manual` | Run from the panel or CLI | ✅ |
| `cron` | `cron = "0 8 * * *"` | ✅ |
| `file-event` | Run on a file create/edit | ⚠️ on backlog |
| `webhook` | Run on HTTP POST | ⚠️ on backlog |

## Step types

- **`ipc`** — call a plugin's IPC handler (anything in the kernel).
- **`shell`** — run a shell command. Output captured; exit code
  becomes a step status.
- **`skill`** — render a skill template; output is the rendered prompt
  or, with `send = true`, the model's response.
- **`ai`** — send a prompt to the AI provider directly.
- **`branch`** — conditional next-step pick.
- **`parallel`** — run a set of steps concurrently.

## Variables

`{{today}}`, `{{now}}`, `{{forge_path}}` are built in. Step outputs
are accessible as `{{steps.<id>.output}}`.

## Run

```bash
nexus workflow list
nexus workflow run "Daily journal"
nexus workflow run ./my.workflow.toml --arg key=value
```

Or open the **Workflows** panel in the shell — every workflow has a
**Run** button and a parameter form.

## Logs

Each run writes a JSONL log to `<forge>/.forge/logs/workflows/<id>.jsonl`
with per-step timing, output, and status. The Workflows panel shows
the latest 20 runs with a status pill.

## Examples

**Index update on file edit** (when file-event triggers ship):

```toml
trigger = { type = "file-event", glob = "**/*.md" }

[[steps]]
type = "ipc"
plugin = "com.nexus.ai"
command = "embed_note"
args = { path = "{{event.path}}" }
```

**Weekly review summary**:

```toml
trigger = { type = "cron", cron = "0 9 * * 1" }

[[steps]]
type = "ai"
prompt = "Summarize the past week's notes in 5 bullets."

[[steps]]
type = "ipc"
plugin = "com.nexus.storage"
command = "create_note"
args = { path = "reviews/{{today}}.md", content = "{{steps[0].output}}" }
```

## When to use which

- **Skill** — single prompt with parameters.
- **Workflow** — multi-step, triggered.
- **Agent** — multi-step with model-driven decisions and tool use.

Workflows are deterministic; agents are not. Reach for a workflow when
the steps are fixed; reach for an agent when the next step depends on
the model's reasoning.
