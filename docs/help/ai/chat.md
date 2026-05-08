# Chat and RAG

The **AI Chat** panel lets you talk to a configured model with
retrieval-augmented generation (RAG) over your forge. The model sees
relevant excerpts from your notes and cites them in its answer.

## Open the panel

Activity bar → **AI Chat** icon. Or palette → "AI: Open chat".

## A first conversation

Type a question:

> What did I write this week about wikilinks?

Nexus:

1. Embeds the query.
2. Runs a vector search over your indexed notes for the top-K most
   similar chunks (default K=8).
3. Sends those chunks plus your query to the configured model as a
   single prompt.
4. Streams the answer with citations like `[^1]` that link back to the
   source notes.

Click any citation to open the cited note at the cited block.

## Sessions

Each chat is a **session**, persisted as JSON in
`<forge>/.forge/chat/sessions/<id>.json`. The session picker (top of
the panel) lets you:

- Resume past sessions.
- Rename or delete sessions.
- Pin sessions you return to often.

Sessions survive restarts; conversation history is preserved.

## Session-level controls

| Control | Effect |
|---|---|
| **Provider** dropdown | Override the default provider for this session |
| **Model** dropdown | Pick a specific model (e.g. claude-opus-4-7, gpt-4-turbo) |
| **Skill** picker | Inject a parameterized prompt template (see [Skills](../advanced/skills.md)) |
| **Files** picker | Pin specific notes to always include in context |
| **Clear** | Wipe history (keeps the session shell) |

## RAG details

Retrieval is dense vector search:

- Indexing: embeddings are precomputed and stored alongside the note
  content. Embedding model is configurable in `.forge/ai.toml`
  (default: a local fastembed model).
- Chunking: notes are chunked at block boundaries with a small
  overlap.
- Recall: top-K cosine-similar chunks are pulled.
- Re-ranking: a lightweight BM25 pass re-ranks the candidates by
  literal term overlap with the query.

## CLI

```bash
nexus ai ask "what did I write this week about wikilinks?"
nexus ai ask "..." --session 1234
nexus ai ask "..." --no-rag           # send the prompt as-is
```

## Embeddings

```bash
nexus ai embed                        # build / refresh embeddings
nexus ai embed --rebuild              # full rebuild
nexus ai status                       # provider, model, embedding count
```

Embeddings update incrementally on file change. The first run on a
large forge can take a few minutes.

## Skills in chat

Pick a skill from the picker; the skill's template renders against your
inputs and the rendered prompt becomes the next user message. Useful
for repeatable workflows ("draft a release note for [feature]"). See
[Skills](../advanced/skills.md).

## Limits

- Context window is the model's, not Nexus's. RAG keeps prompts small,
  but very long sessions can still exceed limits — start a fresh
  session if responses get cut off or sluggish.
- Citations only point to indexed content. Recently created or edited
  notes may take a watcher tick to appear.
