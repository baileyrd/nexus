# Configuring providers

Nexus supports Anthropic Claude, OpenAI, and Ollama out of the box.
Embeddings can be local (fastembed) or cloud. You can configure
multiple providers and pick a default; per-feature overrides let you
mix (e.g. cloud for chat, local for inline completion).

## The config file

`<forge>/.forge/ai.toml`. A minimal config:

```toml
default_provider = "anthropic"

[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"
default_model = "claude-opus-4-7"
```

## Anthropic Claude

```toml
[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"
default_model = "claude-opus-4-7"

[providers.anthropic.models.fast]
id = "claude-haiku-4-5"

[providers.anthropic.models.deep]
id = "claude-opus-4-7"
max_tokens = 8000
```

Get a key at <https://console.anthropic.com>.

## OpenAI

```toml
[providers.openai]
api_key = "${OPENAI_API_KEY}"
default_model = "gpt-4-turbo"
base_url = "https://api.openai.com/v1"   # override for compatible endpoints
```

The `base_url` override means you can point at any OpenAI-compatible
service (Azure OpenAI, OpenRouter, LM Studio, etc.).

## Ollama (local)

[Install Ollama](https://ollama.com), pull a model, then:

```toml
[providers.ollama]
base_url = "http://localhost:11434"
default_model = "llama3.2:3b"
```

```bash
ollama pull llama3.2:3b
ollama pull nomic-embed-text     # if you want local embeddings via Ollama
```

Ollama works with no API key. Useful for local-only forges and for fast
inline completion.

## Embeddings

```toml
[embeddings]
provider = "fastembed"           # default — local, no network
model = "BAAI/bge-small-en-v1.5"

# or:
[embeddings]
provider = "openai"
model = "text-embedding-3-small"

# or:
[embeddings]
provider = "ollama"
model = "nomic-embed-text"
```

Local fastembed is the default. The model downloads on first use
(~100 MB) and runs entirely on your machine.

## Per-feature overrides

```toml
[chat]
provider = "anthropic"

[inline_completion]
provider = "ollama"
model = "llama3.2:3b"

[agent]
provider = "anthropic"
model = "claude-opus-4-7"
```

If you don't override, features use `default_provider`.

## API keys: keyring vs env vars vs plaintext

Order of preference:

1. **OS keyring** (default) — keys never touch disk in plaintext. Set
   with `nexus ai config set anthropic.api_key`.
2. **Environment variable interpolation** — `${ANTHROPIC_API_KEY}` in
   `ai.toml`.
3. **Plaintext in `ai.toml`** — only if you set `NEXUS_NO_KEYRING=1`.
   See [ADR 0009](../../adr/0009-keyring-hard-fail-policy.md).

## Inspect

```bash
nexus ai status
# default provider: anthropic
# default model:    claude-opus-4-7
# embedding model:  fastembed BAAI/bge-small-en-v1.5
# embeddings:       412 / 412 notes
```

## Test

```bash
nexus ai ask "hello, are you online?" --no-rag
```

If the call succeeds you're set. If not, check the keyring entry, then
the env var, then the `ai.toml`.
