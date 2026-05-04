# AI overview and privacy

Nexus has first-class AI features: chat with retrieval over your notes,
inline editor completion, agents that use tools, and skills (parameter-
ized prompt templates). They're all opt-in and route through one
configurable provider.

## What you can do

- **Chat** with your forge — ask a question, get an answer with
  citations to the notes that informed it. See [Chat and RAG](chat.md).
- **Inline complete** at the cursor — `Ctrl+Shift+Space` streams a
  continuation. See [Inline completion](inline-completion.md).
- **Run agents** — multi-step tool-using loops with stepwise approval.
  See [Agents](../advanced/agents.md).
- **Render skills** — fill a parameterized prompt template and send it
  to the model. See [Skills](../advanced/skills.md).

## Providers

Nexus is multi-provider. Configure one or more:

- **Anthropic Claude** (recommended; streaming)
- **OpenAI** (GPT-4 / GPT-3.5)
- **Ollama** (local; no data leaves your machine)
- **Local embeddings** via [fastembed](https://github.com/Anush008/fastembed-rs)

See [Configuring providers](providers.md) to set up keys and pick a
default.

## Privacy model

Nexus does not call any AI provider unless you explicitly trigger a
feature that uses one. There is no telemetry, no background indexing
to a remote service, no automatic embedding upload.

When you do trigger an AI feature:

1. **Chat / inline complete** — the prompt and any retrieved RAG
   chunks are sent to the configured provider.
2. **Embeddings** — by default, embeddings are computed **locally**
   with `fastembed`. You can switch to a cloud embedding model in
   `.forge/ai.toml` if you prefer.
3. **Logging** — every AI call is recorded in
   `.forge/logs/ai-activity.jsonl` for audit. You can review or delete
   the log at any time.

## Privacy mode

Set in `.forge/ai.toml`:

```toml
[privacy]
local_only = true        # refuse cloud providers entirely
disable_embeddings = false
```

With `local_only = true`, only Ollama (and any future local providers)
will run; configured cloud providers return an error.

## API keys

API keys can come from:

1. The OS keyring (default, recommended).
2. An environment variable referenced from `ai.toml`:

   ```toml
   [providers.anthropic]
   api_key = "${ANTHROPIC_API_KEY}"
   ```

3. Plaintext in `ai.toml` (only if `NEXUS_NO_KEYRING=1` is set; not
   recommended).

See [ADR 0009](../../adr/0009-keyring-hard-fail-policy.md) for the
keyring policy.

## Cost and rate limiting

Nexus does not enforce a token budget or rate limit by itself — that's
the provider's responsibility. The shell shows a token counter in the
status bar so you can see what each call cost. The activity log
(`ai-activity.jsonl`) records token counts per call so you can compute
totals.

## Disabling AI entirely

Don't configure a provider. Without one, AI features show a "configure
a provider to enable" message. You can also disable the
`com.nexus.ai` core plugin if you want to be sure no AI code is
loaded.
