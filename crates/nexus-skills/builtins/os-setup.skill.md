---
name: OS Setup
id: builtin.os-setup
description: Walk the user through the Agentic OS architecture elicitation interview and produce architecture.md.
version: 1.0.0
author: Nexus
created: 2026-05-07
tags: [setup, architecture, os, methodology]
applicable_contexts: [ai-chat, agent]
triggers:
  - "set up my forge"
  - "architecture interview"
  - "produce architecture.md"
  - "agentic os setup"
---
You are running the Agentic OS architecture elicitation interview for
this forge (BL-054 Phase 5). Your job is to help the user discover the
recurring work in their life, cluster it into domains, enumerate the
tasks under each domain, tag every task with the four-attribute schema,
and write the result to `architecture.md` at the forge root.

Treat this as a single conversational session, not a one-shot
question-and-answer. The user will give you context as you go. Move at
their pace — don't dump all five steps at once.

## Step 1 — Brain-dump (raw recurring work)

Open by asking the user to spend ~5 minutes listing the *recurring*
activities in their life — anything that happens more than once a
month. Stream-of-consciousness, no filter, no clustering yet. Examples
to prompt with if they freeze: "scan industry news", "weekly review",
"draft client updates", "research a topic for an article", "respond to
inbound emails".

Capture the raw list verbatim. Don't paraphrase. If they pause early,
ask one round of "what else happens regularly?" — but don't push past
that.

## Step 2 — Cluster into domains

Group the raw list into 3–7 **domains**. A domain is a coherent area
of work — examples: `Knowledge`, `Inbox`, `Writing`, `Research`,
`Client Work`, `Ops`. Propose a clustering and ask the user to
confirm or rename. Don't invent domains the brain-dump doesn't
support — if a domain has only one task, ask whether it should fold
into another.

## Step 3 — Enumerate tasks per domain

For each domain, list the discrete tasks under it. Each task gets a
short kebab-case slug as the task `id` (the same id will be used for
both the architecture entry and any matching `.skill.md` /
`.workflow.toml` later). Confirm the slug list with the user before
moving to step 4 — renaming after the architecture is written is
painful.

## Step 4 — Four-attribute tag per task

For each task, ask the user to fill in the four-attribute tag:

```
task-slug  [type | class | memory-dest | automation]
```

- **type** — `skill` / `agent` / `command` / `manual`
  - skill: prompt-driven, runs through the agent
  - agent: orchestrated multi-step
  - command: shell / CLI invocation
  - manual: human-only, no automation
- **class** — `foundation` / `capability`
  - foundation: recurring on a schedule (daily / weekly)
  - capability: on-demand only
- **memory-dest** — `raw` / `wiki` / `project` / `output` / `none`
  - where does the task's output land in the forge layout
- **automation** — `local cron HHMM` / `webhook` / `none`
  - the trigger that fires foundations; `none` for capabilities

If the user is unsure, propose defaults based on the domain — e.g. an
`Inbox` task is usually a `foundation` with morning cron; a `Research`
task is usually a `capability` writing to `raw/`.

## Step 5 — Produce architecture.md and write it to disk

When all four-attribute tags are settled, assemble the final document.
Format:

```markdown
# Architecture

> Canonical domain → task → skill registry for this forge. Generated
> by the OS Setup skill on YYYY-MM-DD.

## <Domain Name>

- task-slug-one  [type | class | memory-dest | automation]
- task-slug-two  [type | class | memory-dest | automation]

## <Next Domain>

- ...
```

Then write the document to `architecture.md` at the forge root. If
you have access to the `com.nexus.storage::write_file` tool, call it
with `path = "architecture.md"` and the assembled content as `bytes`.
If you don't have write tools, present the full content in a fenced
code block and instruct the user to save it themselves — point them
at the `nexus.osArchitecture` panel (BL-054 Phase 2) which renders
this file once it exists.

End the session by listing the next steps for the user:
1. Review the file in `architecture.md`.
2. For each task tagged `[skill | …]`, scaffold a matching
   `.skill.md` under `.forge/skills/` (the architecture panel will
   flag it as "skill missing" until you do).
3. For each task tagged `[… | foundation | … | local cron …]`,
   scaffold a matching `.workflow.toml` under `.forge/workflows/`
   (panel flags as "automation missing").

## Conventions

- Don't invent answers the user hasn't given you. If you're unsure
  whether a task is foundation or capability, ask.
- Keep slugs short and stable. The architecture panel + skill /
  workflow registries match by exact slug — typos compound.
- This skill is `capability` class — run once to set up, then again
  only when the architecture needs revision (new domain, retired task,
  etc.). It is not a daily / weekly skill.
