# Inline completion in the editor

Place the cursor anywhere in a note and press
`Ctrl+I` (or `Cmd+I` on macOS). Nexus grabs the
preceding ~2 KB of text (or the current selection if there is one),
sends it to the configured AI provider, and streams a continuation
into the document.

## Triggering

| Action | Result |
|---|---|
| `Ctrl+I` at the cursor | Continue from the cursor |
| Select a range, then `Ctrl+I` | Replace the selection with a rewrite |
| `Esc` while streaming | Cancel; partial output stays |
| `Ctrl+Z` after | Undo the entire insertion as one transaction |

## What gets sent

- Up to ~2 KB of text immediately before the cursor (paragraph
  boundaries respected so the prompt doesn't start mid-word).
- The current note's frontmatter (so the model can see the title and
  tags).
- **Not** the rest of the forge — inline completion does **not** use
  RAG. For RAG-augmented answers, use the [Chat panel](chat.md).

## Streaming

Tokens arrive incrementally and appear in the editor as they're
generated. The cursor follows the insertion. You can keep editing
elsewhere in the document; the streaming inserter watches the original
position.

## Provider

By default, inline completion uses the same provider as chat. Override
in `.forge/ai.toml`:

```toml
[inline_completion]
provider = "ollama"        # use local model for fast turnaround
model = "llama3.2:3b"
max_tokens = 200
temperature = 0.3
```

A small local model like `llama3.2:3b` over Ollama is fast enough to
feel like autocomplete.

## When it's a good idea

- Continuing a paragraph you've started.
- Filling in boilerplate (frontmatter, code-block scaffolds).
- Rewriting a selected sentence in a different tone.

## When it's not

- Answering questions about your forge — use chat.
- Long-form generation — start a chat, ask the model to draft, then
  paste.
- Code that needs to compile — the model has no compiler. Use a
  language-server-aware editor for serious code.

## Privacy

Same provider routing as chat. The inline completion text and your
recent paragraph are sent to the configured provider. The call is
logged in `.forge/logs/ai-activity.jsonl`. See
[AI overview / privacy](overview.md).
