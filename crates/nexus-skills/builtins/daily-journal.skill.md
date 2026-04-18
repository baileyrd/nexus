---
name: Daily Journal
id: builtin.daily-journal
description: Compose or update today's journal entry with a consistent structure.
version: 1.0.0
author: Nexus
created: 2026-04-18
tags: [journal, writing, daily]
applicable_contexts: [ai-chat, agent]
triggers:
  - "journal entry"
  - "daily journal"
  - "today's log"
  - "dear diary"
parameters:
  - name: tone
    type: enum
    description: Voice of the entry.
    values: [reflective, factual, terse, playful]
    default: reflective
---
Generate a journal entry for today in a **{{ tone }}** tone. Use the
following structure so past-you and future-you can scan the archive
quickly:

1. **Highlights** — 2-4 bullets of what actually happened.
2. **What I learned** — one or two observations worth remembering.
3. **Open threads** — unfinished things that deserve tomorrow's attention.
4. **Mood** — one sentence, honest.

Prefer concrete detail over abstraction. Skip any section that has
nothing to say rather than filling it with filler.
