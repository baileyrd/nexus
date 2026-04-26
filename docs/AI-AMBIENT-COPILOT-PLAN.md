# Ambient Copilot — UX Integration Plan

> The *feel* of how AI is woven into Pieces.app, mapped onto Nexus.
> Captured 2026-04-26.
>
> Companion to:
> - [`AI-INTEGRATION-DIRECTIONS.md`](AI-INTEGRATION-DIRECTIONS.md) —
>   broad menu of integration directions.
> - [`AI-MEMORY-LAYER-PLAN.md`](AI-MEMORY-LAYER-PLAN.md) — capture +
>   memory side, inspired by Pieces' Drive.
>
> This doc focuses on the **interface** side: how the AI is summoned,
> displayed, and made aware of context across surfaces.

## Framing

The distinctive thing about Pieces' AI integration isn't any single
feature — it's that the copilot is **ambient**. Always there, always
knowing what you're doing, accessible by the smallest gesture.
Today's Nexus AI is the opposite: open the panel, type a question,
hit send. The user has to *go* to the AI; in Pieces, the AI comes to
the user.

The patterns below are what produce that ambient feel.

## The ten patterns

### 1. Cmd+I anywhere

A global shortcut that opens a mini copilot overlay over whatever
surface the user is on, **pre-loaded with that surface's context**.

- In the editor → overlay anchored to the cursor, current file +
  selection auto-attached.
- In a Base → scoped to that base's records.
- In Canvas → scoped to the visible cards.
- In Search → seeded with the current query.

One shortcut, different context per surface.

### 2. Visible context chips

The copilot panel always shows what it can "see" right now as small
chips at the top:

```
📄 ARCHITECTURE.md   ✂️ selection (42 lines)   🕐 last 3 files
```

Click a chip to remove it. Drag a file from the sidebar in to add it.
Today Nexus chat has no visible context surface — the user has no
idea what the AI is grounding on.

### 3. Per-thread model switcher

Top-right of every chat: a dropdown showing your configured models
(local Ollama, cloud Anthropic, cloud OpenAI). One click switches
mid-conversation. Pieces does this; it makes the local-vs-cloud
tradeoff *visible* without a settings trip. The `set_config`
plumbing already shipped supports this — just needs UI.

### 4. Inline ghost suggestions

As you type prose, faint gray ghost text predicts the next phrase
(like GitHub Copilot but for markdown). Tab to accept, Esc to
dismiss. Cheap with a small local model.

This is the **highest-leverage single feature** in Pieces — the AI
is *visibly* helping you write without you asking.

### 5. Right-click → AI Actions

Selection in the editor → right-click → submenu:

- Rewrite
- Explain
- Continue
- Summarize
- Translate
- Fix grammar

Each is a one-shot prompt that streams a result inline with accept/
reject. Same actions as the inline-editor item from the directions
doc, but exposed via context menu instead of palette.

### 6. Ambient suggestions in margins

When you write a heading or start a new section, faded "related"
suggestions appear in the right margin — links to similar notes,
draft starters, recent captures. Click to insert. Pieces' "Related"
panel.

### 7. Activity timeline

A persistent strip (or right-panel tab) showing what you did today:
files edited, terminal commands run, captures saved, chats started.
The copilot can answer "summarize what I worked on today" against it.

### 8. Cited everything

Every AI response renders source chips beneath it — files, captures,
web pages — that open the source on click. Today Nexus chat has this
for RAG sources but not for inline actions. Should be uniform across
every AI response in the app.

### 9. Saved threads as first-class objects

Chat sessions today live in a drawer. Pieces treats them as
documents — pinnable, taggable, searchable, openable as tabs.

Proposal: make a chat session a viewType. It gets a tab like any
markdown file, lives in `.forge/chat/`, can be linked from notes
(`[[chat:auth-discussion]]`).

### 10. Workspace-scoped persona

A `system-prompt.md` at the workspace root that sets the AI's role
for this workspace.

- "This is a research journal — answer cautiously, cite sources."
- "This is a code project — be terse and code-first."

One file, applies everywhere. Editable like any other note.

## Build phasing

The two highest-leverage items by far are **#1 (Cmd+I overlay)** and
**#4 (inline ghost suggestions)**. Together they're the difference
between "an AI app" and "an app where AI is everywhere."

| Phase | Theme | Items | Estimate |
|---|---|---|---|
| A | Visible context | #2, #3 | 1–2 days. All UI, infrastructure exists. |
| B | Ambient invocation | #1, #5 | 3–5 days. Reuses streaming runtime; new overlay component. |
| C | Inline assistance | #4, #6 | ~1 week. New CodeMirror extension, debounced background calls. |
| D | Workspace memory | #7, #9, #10 | ~1 week. |

After **Phase B** the difference is already obvious — the AI stops
being a place you visit and becomes something you summon.

## Why this is reachable for Nexus

Most of the plumbing already exists:

- Streaming AI runtime — `nexus-ai`.
- RAG retriever — `nexus-ai::rag::query`.
- Runtime provider switching — `set_config` (just landed).
- Plugin sandbox + slot system for new UI surfaces.
- CodeMirror with extension hooks for ghost text + decorations.
- Workspace event bus for activity tracking.

The work is mostly **UI surface and event wiring**, not new
infrastructure.
