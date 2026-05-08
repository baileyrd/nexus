# ADR 0018: Local Embedding Backend — fastembed-rs

**Date:** 2026-04-28
**Status:** Accepted

## Context

`crates/nexus-ai/src/embedding.rs` defines a single trait:

```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AiError>;
    fn dimension(&self) -> usize;
}
```

Two implementations ship today: Ollama (`crates/nexus-ai/src/ollama.rs`,
default model `nomic-embed-text`) and OpenAI. Both require a network
round-trip — the Ollama daemon may be local, but it's still an out-of-
process HTTP call to a separately installed binary the user has to
manage.

Storage is already solved: `crates/nexus-ai/src/vectorstore.rs` calls
`com.nexus.storage::vector_insert` / `vector_search` over IPC, which
runs in `nexus-storage` against the forge SQLite database. This ADR is
purely about the embedding *generator*, not vector storage.

The original BL-019 entry called BL-019 "nice-to-have." After the
2026-04-28 plan refresh BL-019 gates **nine downstream tracks**:
BL-038 (citations), BL-039 (auto-link suggestions), BL-040 (semantic
search), BL-041 (background indexing daemon), BL-044 (recall hotkey),
BL-045 (auto-enrichment on save), BL-047 (scheduled digests), plus
retrieval variants of BL-010 / BL-011 / BL-034. Choosing a backend
that bloats the binary, ships poorly cross-platform, or produces
sub-baseline embeddings will compound across every consumer.

## Decision

**Use `fastembed-rs` as the local embedding backend.**

Default model: **BGE-small-en-v1.5 (INT8 quantized)** — 33 MB on disk,
~120 MB resident, 384-dim vectors, MTEB average ~62 (within 1-2 points
of the FP32 baseline).

### Rationale axes

| Axis | fastembed-rs | candle | llama.cpp / sqlite-lembed |
|---|---|---|---|
| Model quality (MTEB avg) | ~62 (BGE-small INT8) | ~62 (same model FP32, larger) | ~58 (gguf-quantized) |
| RAM resident (idle / under load) | ~120 MB / ~250 MB | ~150 MB / ~300 MB | ~200 MB / ~400 MB |
| Cold-start time | ~1.2 s first call | ~1.5 s first call | ~2.0 s first call |
| Cross-platform binary cost | +30–50 MB ORT | +10–20 MB pure Rust | +40–80 MB C++ runtime |
| Workspace compile-time hit | Low (ORT linkage) | High (large dep tree) | Medium (C++ rebuild) |
| Out-of-box model registry | Curated (BGE / mxbai / nomic) | None — wire each model | gguf zoo, less curated |
| Production maturity | Used in qdrant / chromadb embeddings | Younger, fewer prod deployments | Mature for LLM, less so for embed |
| License | Apache 2.0 | Apache 2.0 / MIT | MIT |

### Implementation outline

1. New module `crates/nexus-ai/src/local_embedding.rs` implements
   `EmbeddingProvider` over `fastembed::TextEmbedding`.
2. Adds an optional Cargo feature `local-embeddings` so the dependency
   doesn't compile when only remote providers are needed (the WASM
   community-plugin tier won't pull this in).
3. Model files cached under `~/.cache/nexus/fastembed/<model>/` —
   downloaded on first use, hash-verified, mirrorable.
4. New config key `[ai] local_embedding_model = "bge-small-en-v1.5-int8"`
   in `crates/nexus-ai/src/config.rs`. Falls back to remote if the key
   is unset (non-breaking).
5. Cache layer: a `dashmap::DashMap<u64, Vec<f32>>` keyed by SHA-256 of
   the input text; bounded by `[ai] embedding_cache_max_entries`
   (default 50_000). Cuts repeated chunk-recompute on indexing reruns.

### Model swap-out

The Cargo dependency surface is the same for any fastembed-supported
model. To swap (e.g. to `mxbai-embed-large-v1` for higher quality at
~300 MB), users change one config key. No code change. We ship with
`bge-small-en-v1.5-int8` because it's the best size-quality knee for
personal-tool use; power users with the RAM headroom can upgrade.

## Alternatives considered

### A. `candle`

HuggingFace's pure-Rust ML framework. Same model quality
(BGE-small loaded as F32) but:

- **Compile-time cost.** Pulls `candle-core`, `candle-nn`,
  `candle-transformers` and a tokenizer crate into the workspace —
  doubles `cargo build -p nexus-ai` cold-build time in our local
  benchmarks.
- **No curated model registry.** Each model needs hand-wired
  config + tokenizer + weight format. fastembed-rs's model enum is a
  one-line swap.
- **Production immaturity.** Fewer real deployments using candle for
  embeddings; debug story is thinner if a model misbehaves under load.
- **No INT8.** F32 weights are larger on disk and slower at inference
  for the same quality.

The pure-Rust appeal is real (no ORT linkage, smaller binary by
~20-30 MB). It's the right pick if ORT becomes a deployment
problem on a target platform we don't currently support — revisit
when shipping web/mobile (PRD-17 deferred).

### B. llama.cpp / sqlite-lembed

The "sqlite-vec gguf path" framing in the original BL-019 entry
points here: `sqlite-lembed` is the sister project to `sqlite-vec`
that bundles llama.cpp for in-database embedding generation.

Rejected:

- llama.cpp is optimized for **LLM** inference; embedding is a
  secondary path with less attention from the upstream maintainers.
- C++ build adds platform-specific deployment complexity (CMake,
  linker flags, occasional Windows build breaks) — a worse story than
  ORT's pre-built binaries.
- Embedding quality of available gguf-quantized models trails the
  Apache-licensed BGE family by 3-5 MTEB points.
- Couples embedding generation to the storage layer (sqlite-lembed
  loads the model inside the SQLite extension). Conflicts with the
  microkernel invariant: storage is supposed to own the SQLite
  connection, not the inference engine. Adding inference there
  breaks the "kernel never depends on a subsystem" rule indirectly
  by making storage co-deploy a model runtime.

### C. Stay remote-only (Ollama / OpenAI)

Rejected. The two failure modes:

- **Privacy / offline use.** Forges may contain personal notes the
  user does not want sent anywhere. Even local Ollama is a
  separately-installed daemon the user has to set up.
- **Latency.** Remote embeddings make BL-040 (semantic search) and
  BL-044 (recall hotkey) feel slow. Local embeddings on a quantized
  small model finish in <50 ms per query on commodity laptop CPU.

### D. Hybrid: ship fastembed-rs, fall back to remote when offline

This is what we end up with naturally. The trait is the
abstraction; users can configure remote, local, or both with
preference rules. The default for new forges is local
(fastembed-rs). Remote remains for users who want a particular
hosted model.

## Consequences

### Positive

- **Fully offline forges.** No network call needed for embeddings;
  unblocks every BL-019 consumer for private / air-gapped use.
- **Latency floor.** <50 ms / query on commodity CPU; BL-040 +
  BL-044 land as snappy interactions.
- **One config key to swap models.** Power users can upgrade to
  `mxbai-embed-large-v1` (or anything else fastembed supports) with
  zero code change.
- **No migration burden on existing remote users.** The trait is the
  abstraction; existing Ollama / OpenAI callers keep working
  unchanged. Local is opt-in via config.
- **Cache reuse.** `dashmap` cache is shared across BL-041 indexer
  and on-demand embedders, cutting 60-90% of recomputes on a typical
  re-index.

### Negative

- **+30-50 MB binary.** The ONNX Runtime dynamic library ships in
  every Nexus release that compiles `local-embeddings`. Mitigated by
  the Cargo feature flag — minimal builds (CLI-only, headless) can
  drop it.
- **Model download on first use.** ~33 MB one-time download for
  BGE-small. Acceptable; future versions can pre-bake the model into
  the installer for users who want zero-network onboarding.
- **Cargo feature surface grows.** `local-embeddings` is one more
  feature flag to test; CI matrix expansion is one more axis.
- **English-default.** BGE-small-en is English. Multilingual users
  configure `bge-m3` (560 MB, multilingual) or similar. Should be
  documented; not a blocker.

### Neutral

- The `EmbeddingProvider` trait is unchanged. Local backend is just
  a third implementor.
- Vector storage continues to live in `nexus-storage` via the
  existing `vector_insert` / `vector_search` IPC handlers. This ADR
  has no impact on the storage side.
- ADR 0008 (tech-stack defaults) needs a one-line addendum noting
  fastembed-rs as the embedding default.

## Implementation sketch

1. Add `fastembed = "<latest>"` under a `local-embeddings` Cargo
   feature in `crates/nexus-ai/Cargo.toml`.
2. Create `crates/nexus-ai/src/local_embedding.rs` with
   `LocalEmbedding { inner: TextEmbedding, dim: usize, cache: DashMap<u64, Vec<f32>> }`
   implementing `EmbeddingProvider`.
3. Wire model selection through `AiConfig::local_embedding_model`
   (default `"bge-small-en-v1.5-int8"`). Resolver in `config.rs`.
4. Cache key: `xxhash3_64` of the input text (already in our deps via
   tantivy). Eviction: simple LRU on entry count.
5. Tests:
   - First-call cold-start + second-call cache hit.
   - Dimension matches storage schema (`vector_insert` rejects
     mismatched dim — we have a test in `nexus-storage` that pins this).
   - Round-trip embed → insert → search returns the inserted chunk.
6. Update `docs/adr/0008-tech-stack-defaults.md` with the embedding
   default.

## Phase mapping

This ADR unblocks BL-019 implementation in Phase 4. The four BL-019
sub-tasks listed in the implementation plan map cleanly:

1. **Backend impl** — this ADR + the `local_embedding.rs` module.
2. **`EmbeddingModel` trait + cache** — already trait-shaped; cache
   is the additive change.
3. **RAG wire-up** — point `nexus-ai`'s rag.rs at the local provider
   when configured.
4. **Batch indexer hook for BL-041** — expose a `embed_batch`
   throughput-optimized path on the local provider (skip per-call
   overhead for >1k chunk batches).

## References

- `crates/nexus-ai/src/embedding.rs` — `EmbeddingProvider` trait.
- `crates/nexus-ai/src/vectorstore.rs` — IPC client into storage's
  vector handlers (storage already owns persistence).
- `crates/nexus-ai/src/ollama.rs:18` — current default
  `nomic-embed-text` for the remote path.
- `crates/nexus-ai/src/openai.rs` — remote OpenAI embeddings.
- BL-019 entry in `docs/PRDs/BACKLOG.md` (lines surrounding the
  "promoted from nice-to-have" cross-reference).
- ADR 0008 — tech-stack defaults (gets a one-line update).
- fastembed-rs upstream: `https://github.com/Anush008/fastembed-rs`
  (Apache 2.0).
