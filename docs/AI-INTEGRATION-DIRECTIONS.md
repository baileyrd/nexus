# AI Integration Directions

> **Status: exploratory.** Indexed in [PRDs/BACKLOG.md → "Future directions"](PRDs/BACKLOG.md#future-directions-exploratory-not-phased). This file is the design rationale; promote a direction into a scoped backlog item when work begins.

> Scratch document: directions for deeper AI integration into Nexus,
> beyond the existing right‑panel chat. Captured 2026‑04‑26.

## Where we are today

The infrastructure is already mostly in place:

- `nexus-ai` — providers (Anthropic / OpenAI / Ollama), RAG with
  vectorstore, chat sessions, runtime `set_config`.
- `nexus-agent` — agent plugin scaffold.
- `nexus-mcp` — MCP plugin (consuming external MCP servers).
- `nexus-skills` — skills crate scaffold.
- `nexus-workflow` — workflow plugin scaffold.

What's missing is **integration into the editing surface itself**
rather than another side panel.

## Directions

Ordered roughly by impact-per-effort.

### 1. Inline editor actions  *(highest value, smallest scope)*

Selection-based commands: **rewrite, summarize, explain, continue,
translate, fix grammar**. Cmd+K opens a contextual prompt with the
selection pre-attached. Response streams back into the document with a
diff-style accept/reject affordance.

CodeMirror already exposes the selection range and the AI plugin
already has a streaming runtime — the missing piece is a small overlay
component plus 4–5 prompt templates.

### 2. Auto-link / backlink suggestions while typing

On a heading or bullet, the global graph index + an embedding lookup
can surface `[[likely-link]]` candidates inline. We already index
everything for RAG; reusing those embeddings for "did you mean to link
X?" is essentially free.

### 3. Semantic search that answers, not just lists

The current search is lexical. Replace (or augment) it with the same
RAG pipeline the chat uses — query → top-k chunks → grounded answer
with citation chips that open the source file. The plumbing already
exists in `nexus-ai::rag::query`; just needs a different surface in
the search panel.

### 4. Per-surface chat handles

"Ask about this Base", "ask about this canvas", "ask about this
folder". Same chat view, scoped retrieval. `BasesView` / `CanvasView`
each emit a context object the chat plugin scopes RAG against. Cheap
once the RAG retriever takes a path filter.

### 5. Skills as authored prompts

`nexus-skills` is scaffolded but unused for AI. Treat each `.skill`
file as a callable prompt template — agents can invoke them, the
command palette surfaces them, the user authors them in markdown.
Turns the workspace into the user's personal prompt library.

### 6. Tool-using agent loop on `nexus-agent`

The agent plugin exists but tool-calling isn't wired yet. With
`set_config` now landed for runtime credentials, the next step is
exposing **file read/write/search/grep** as Anthropic/OpenAI tool
definitions and giving the agent a turn loop. That's the unlock for
"rename every reference to X across my notes" or "draft a weekly
review from this folder."

### 7. Expose Nexus as an MCP server

Inverse of consuming MCP — let external clients (Claude Desktop,
IDEs, the CLI) read/query the workspace through MCP. `nexus-mcp` is
the foundation; need to publish a stdio MCP server that proxies to
the kernel. Lets you ask Claude in another window about your notes
without leaving them.

### 8. Background indexing + scheduled digests

Embed on save (already exists) + a scheduled summarization that
produces a "what changed this week" note. Useful for journals,
research logs, project workspaces.

## Recommended ordering

1. **Inline editor actions (#1)** — highest visibility, reuses
   everything we just wired for the settings UX.
2. **Semantic search (#3)** — second-best bang/buck since the
   retrieval pipeline already exists.
3. **Per-surface chat (#4)** — small extension of the chat view that
   makes Bases / Canvas / Folder views feel AI-native.
4. **Tool-using agent (#6)** — once the inline actions reveal what
   tools are most valuable, the agent loop becomes a natural
   follow-up.

The remaining items (auto-link suggestions, skills, MCP server,
digests) are valuable but better tackled after the core editing
integration lands and reveals real usage patterns.
