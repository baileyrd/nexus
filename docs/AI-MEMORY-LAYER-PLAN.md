# Nexus as a Personal AI Memory Layer

> Direction inspired by [Pieces.app](https://pieces.app) — make Nexus
> a personal AI memory layer with capture from everywhere, contextual
> recall, and a copilot that has access to your full history. Captured
> 2026‑04‑26.
>
> Companion to [`AI-INTEGRATION-DIRECTIONS.md`](AI-INTEGRATION-DIRECTIONS.md).

## Framing

Pieces is fundamentally about **personal memory**: capture snippets
from anywhere (IDE, browser, terminal, clipboard), enrich them
automatically, and recall them through semantic search + a copilot
that has full history. That's a meaningfully different shape than
"another chat panel."

## Mapping Pieces onto Nexus today

| Pieces capability | Nexus equivalent today | Gap to close |
|---|---|---|
| Long-term memory of code/text | Workspace of markdown notes | Auto-capture from outside the app |
| Semantic search over memory | RAG vectorstore (`nexus-ai`) | Make it the primary search path |
| Auto-titling / tagging / descriptions | None | Embed-and-classify pipeline on save |
| Workstream context (what you're doing now) | None | Track active file + recent activity |
| Cross-surface chat with memory access | Right-panel chat with RAG | Make context implicit (current file, selection) |
| Local-first AI | Ollama provider supported | Surface privacy story; better local-model defaults |
| Snippet runner / code-aware | Markdown-only today | Code blocks as first-class with language detection |

## Build plan — six pieces in order

### 1. Quick Capture  *(the foundation)*

Without this, nothing else matters. System-wide hotkey (e.g.
Cmd+Shift+Space) drops whatever's on the clipboard / currently
selected into a `captures/` folder in the active workspace.
Auto-titled, auto-tagged, auto-embedded. Comes back as a regular
markdown note — searchable, linkable, editable.

**Implementation:** Tauri global shortcut + a tiny `nexus-capture`
plugin that writes the markdown and triggers the existing
index-on-save pipeline. ~200 lines of Rust + ~100 of TS.

### 2. Auto-enrichment on save

Whenever a note (or capture) is written, run a small LLM pass that
fills in frontmatter: `title`, `tags[]`, `summary`, `related[]`
(links to similar existing notes). Cheap with a local Ollama model;
runs in background; user can override.

This is what makes captures *findable* later without manual filing.

### 3. Recall hotkey

A second global hotkey (e.g. Cmd+Shift+K) opens an overlay anywhere —
even when Nexus isn't focused — that does semantic search across
captures + notes and lets you paste / copy / insert a result. This is
the "I know I saved that command somewhere" workflow.

Reuses the existing RAG retriever; just needs an always-available
overlay window (Tauri supports this).

### 4. Implicit context for chat

Instead of the chat being a blank panel, it knows about:

- The active file (auto-included in context).
- The current selection.
- The last ~5 files the user touched.

So "what was the auth pattern I used?" works without spelling out
which file. The chat plugin already gets workspace events via
`eventBus`; just needs to feed them as system context.

### 5. Code-aware capture + run

Code blocks captured from anywhere get language-detected,
syntax-highlighted, and runnable inline (the `nexus-terminal` plugin
can host this). Pieces leans heavily on this for developer workflows.

### 6. Browser extension / IDE bridge

The hardest one, but what unlocks the "everywhere" feel of Pieces.

- Browser extension that posts to a local IPC port (Tauri can host an
  HTTP/WS server).
- VS Code extension same model.
- Both write into the captures folder via IPC.

Multi-week build. The first three items above give ~80% of the
Pieces feel within Nexus itself, with no out-of-app integration
required.

## Why Nexus could be better than Pieces

One real differentiator: **everything is plain markdown in your
filesystem.** Pieces puts captures in their own opaque store. Nexus
captures would just be notes — versionable, editable, linkable from
your existing workflow, exportable, openable in any editor. That's a
meaningful story for users who already think in markdown.

## Suggested first move

Build **#1 + #2** as a single phase: capture + auto-enrichment.
End-to-end:

1. Hit hotkey anywhere → snippet lands in workspace.
2. Seconds later it has a title, tags, and a summary.
3. It's searchable in chat and global search.

Estimated 1–2 days using infrastructure that already exists
(`nexus-ai` runtime, RAG pipeline, settings-driven provider config
that just landed).
