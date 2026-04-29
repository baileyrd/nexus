//! Local embedding provider backed by `fastembed-rs` (BL-019, ADR 0018).
//!
//! Compiled only when the `local-embeddings` Cargo feature is enabled.
//! See [`LocalEmbedding`] for the [`crate::EmbeddingProvider`] implementor
//! and the in-process [`DashMap`](dashmap::DashMap) cache layer that
//! avoids recomputing embeddings for repeated input chunks across
//! re-indexing runs.
//!
//! # Model selection
//!
//! Default model is `bge-small-en-v1.5-int8` (33 MB on disk, 384-dim).
//! ADR 0018 documents the rationale; power users can swap to
//! `mxbai-embed-large-v1` or any other fastembed-supported model via the
//! `local_embedding_model` config key with no code change.
//!
//! # Caching
//!
//! Inputs are hashed with `xxhash3_64` and cached by the resulting
//! `u64`. The cache is bounded by `embedding_cache_max_entries`
//! (default `50_000`). Eviction is a coarse "drop everything when over
//! budget" sweep — fine-grained LRU isn't worth the bookkeeping for an
//! in-process cache that pays for itself within a single re-index.
//!
//! # Test seam
//!
//! The cache logic is exercised through [`embed_with_cache`], which
//! takes the underlying batch embedder as a closure. Tests stub the
//! closure with deterministic vectors so the full unit-test suite can
//! run without downloading the BGE weights from `HuggingFace`.

#![cfg(feature = "local-embeddings")]

use std::sync::Mutex;

use async_trait::async_trait;
use dashmap::DashMap;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use xxhash_rust::xxh3::xxh3_64;

use crate::embedding::EmbeddingProvider;
use crate::error::AiError;

/// Default model identifier — see ADR 0018 for rationale.
pub const DEFAULT_LOCAL_MODEL: &str = "bge-small-en-v1.5-int8";

/// Default upper bound for the cache. Tuned for a typical re-indexing
/// run on a personal forge (50 k chunks ≈ 76 MB at 384 dims of f32).
pub const DEFAULT_CACHE_MAX_ENTRIES: usize = 50_000;

/// Threshold above which a batch skips cache lookup and goes straight
/// to the bulk embedder. Reading + writing the cache for very large
/// batches costs more than the recomputation savings on the rare
/// duplicate.
pub const BATCH_CACHE_BYPASS_THRESHOLD: usize = 1_000;

/// Local embedding provider backed by fastembed-rs.
///
/// Constructed via [`LocalEmbedding::new`] (loads the model on the
/// caller's thread; for use from async contexts wrap in
/// `tokio::task::spawn_blocking`). The wrapped [`TextEmbedding`] is
/// re-entrant; this struct owns it directly.
pub struct LocalEmbedding {
    // fastembed's `TextEmbedding::embed` takes `&mut self` because the
    // underlying ORT session holds non-Sync mutable state during a
    // forward pass. Wrap in a Mutex so the EmbeddingProvider trait can
    // expose `&self` to callers; contention is bounded — callers should
    // batch large input lists in a single call rather than fan out
    // single-text invocations across threads.
    inner: Mutex<TextEmbedding>,
    dim: usize,
    cache: DashMap<u64, Vec<f32>>,
    cache_max_entries: usize,
}

impl LocalEmbedding {
    /// Construct a [`LocalEmbedding`] using the named model.
    ///
    /// Recognised model identifiers map to fastembed's [`EmbeddingModel`]
    /// enum via [`map_model`]. Unknown identifiers return an
    /// [`AiError::Provider`] so callers can fall back to a remote
    /// provider rather than panicking.
    ///
    /// First-call cost: fastembed downloads the model weights from the
    /// ``HuggingFace`` mirror to `~/.cache/fastembed/<model>/` (~33 MB for
    /// the default). Subsequent calls reuse the on-disk cache.
    ///
    /// # Errors
    ///
    /// Returns [`AiError::Provider`] when the model identifier is
    /// unknown or the underlying `fastembed::TextEmbedding::try_new`
    /// call fails (e.g. unable to download the model weights, missing
    /// `libonnxruntime`).
    pub fn new(model_name: &str) -> Result<Self, AiError> {
        Self::with_capacity(model_name, DEFAULT_CACHE_MAX_ENTRIES)
    }

    /// Same as [`LocalEmbedding::new`] with a caller-controlled cache
    /// budget. Pass `0` to disable caching.
    ///
    /// # Errors
    ///
    /// Same as [`LocalEmbedding::new`].
    pub fn with_capacity(model_name: &str, cache_max_entries: usize) -> Result<Self, AiError> {
        let model = map_model(model_name)?;
        let dim = model_dimension(&model);
        let inner = TextEmbedding::try_new(InitOptions::new(model))
            .map_err(|e| AiError::Provider(format!("fastembed init failed: {e}")))?;
        Ok(Self {
            inner: Mutex::new(inner),
            dim,
            cache: DashMap::new(),
            cache_max_entries,
        })
    }

    /// Number of cached `(text -> vector)` entries currently held.
    /// Test + observability hook; not part of the trait surface.
    #[must_use]
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// Throughput-optimized batch path used by the BL-041 background
    /// indexer. Skips the cache for batches over
    /// [`BATCH_CACHE_BYPASS_THRESHOLD`] entries (the per-key `DashMap`
    /// touch costs more than the rare re-embed saves at that scale).
    ///
    /// For smaller batches this is identical to the [`EmbeddingProvider`]
    /// trait method.
    ///
    /// # Errors
    ///
    /// Returns [`AiError::Provider`] if the inner mutex is poisoned or
    /// fastembed's batch embed call fails.
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AiError> {
        if texts.len() > BATCH_CACHE_BYPASS_THRESHOLD {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| AiError::Provider("local embedding mutex poisoned".to_string()))?;
            return guard
                .embed(texts.iter().map(String::as_str).collect::<Vec<_>>(), None)
                .map_err(|e| AiError::Provider(format!("fastembed embed failed: {e}")));
        }
        embed_with_cache(&self.cache, self.cache_max_entries, texts, |missing| {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| AiError::Provider("local embedding mutex poisoned".to_string()))?;
            guard
                .embed(missing.iter().map(String::as_str).collect::<Vec<_>>(), None)
                .map_err(|e| AiError::Provider(format!("fastembed embed failed: {e}")))
        })
    }
}

#[async_trait]
impl EmbeddingProvider for LocalEmbedding {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AiError> {
        // fastembed's embed call is synchronous + CPU-bound. We don't
        // spawn_blocking here because callers in nexus-ai already drive
        // this from blocking contexts (the bg-indexer track schedules
        // its own threadpool). If a future caller needs a Tokio-friendly
        // wrapper, wrap LocalEmbedding in spawn_blocking at the call site.
        self.embed_batch(texts)
    }

    fn dimension(&self) -> usize {
        self.dim
    }
}

/// Compute embeddings for `texts`, consulting `cache` first and only
/// invoking `fetch` for inputs that aren't already cached. Hits and
/// misses are interleaved in input order.
///
/// Pulled out as a free function so unit tests can exercise the cache
/// logic against a closure-based stub embedder, avoiding the
/// `HuggingFace` weight download that real fastembed instantiation
/// triggers.
fn embed_with_cache<F>(
    cache: &DashMap<u64, Vec<f32>>,
    cache_max: usize,
    texts: &[String],
    fetch: F,
) -> Result<Vec<Vec<f32>>, AiError>
where
    F: FnOnce(&[String]) -> Result<Vec<Vec<f32>>, AiError>,
{
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let mut keys: Vec<u64> = Vec::with_capacity(texts.len());
    let mut missing_idx: Vec<usize> = Vec::new();
    let mut missing_texts: Vec<String> = Vec::new();
    let mut out: Vec<Option<Vec<f32>>> = Vec::with_capacity(texts.len());

    for (i, text) in texts.iter().enumerate() {
        let key = xxh3_64(text.as_bytes());
        keys.push(key);
        if cache_max > 0 {
            if let Some(hit) = cache.get(&key) {
                out.push(Some(hit.clone()));
                continue;
            }
        }
        out.push(None);
        missing_idx.push(i);
        missing_texts.push(text.clone());
    }

    if !missing_texts.is_empty() {
        let fetched = fetch(&missing_texts)?;
        if fetched.len() != missing_texts.len() {
            return Err(AiError::Provider(format!(
                "embed batch returned {} vectors for {} inputs",
                fetched.len(),
                missing_texts.len()
            )));
        }
        if cache_max > 0 && cache.len() >= cache_max {
            cache.clear();
        }
        for (slot, vec) in missing_idx.into_iter().zip(fetched.into_iter()) {
            if cache_max > 0 {
                cache.insert(keys[slot], vec.clone());
            }
            out[slot] = Some(vec);
        }
    }

    Ok(out
        .into_iter()
        .map(|opt| opt.expect("every slot filled by cache or fetch"))
        .collect())
}

fn map_model(name: &str) -> Result<EmbeddingModel, AiError> {
    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        // Default — INT8 quantized, 384-dim, 33 MB.
        "bge-small-en-v1.5-int8" | "bge-small-en-v1.5-q" | "" => {
            Ok(EmbeddingModel::BGESmallENV15Q)
        }
        "bge-small-en-v1.5" => Ok(EmbeddingModel::BGESmallENV15),
        "bge-base-en-v1.5" => Ok(EmbeddingModel::BGEBaseENV15),
        "bge-large-en-v1.5" => Ok(EmbeddingModel::BGELargeENV15),
        "mxbai-embed-large-v1" => Ok(EmbeddingModel::MxbaiEmbedLargeV1),
        "nomic-embed-text-v1.5" => Ok(EmbeddingModel::NomicEmbedTextV15),
        "all-mini-lm-l6-v2" | "all-minilm-l6-v2" => Ok(EmbeddingModel::AllMiniLML6V2),
        other => Err(AiError::Provider(format!(
            "unknown local embedding model: {other}"
        ))),
    }
}

fn model_dimension(model: &EmbeddingModel) -> usize {
    // Default falls through to 384 (BGE-small / MiniLM family + any
    // future fastembed variant we haven't catalogued — callers verify
    // via the `dimension()` trait method).
    match model {
        EmbeddingModel::BGEBaseENV15 | EmbeddingModel::NomicEmbedTextV15 => 768,
        EmbeddingModel::BGELargeENV15 | EmbeddingModel::MxbaiEmbedLargeV1 => 1024,
        _ => 384,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canned_vec(text: &str) -> Vec<f32> {
        // Deterministic stub vector: byte-length + first-byte fan-out.
        // Lets tests assert that the right input mapped to the right
        // output without knowing anything about the embedding model.
        #[allow(clippy::cast_precision_loss)] // test fixture; bytes are small
        let len = text.len() as f32;
        let first = f32::from(text.as_bytes().first().copied().unwrap_or(0));
        vec![len, first, 1.0, 2.0]
    }

    #[test]
    fn cache_hit_skips_fetch_call() {
        let cache: DashMap<u64, Vec<f32>> = DashMap::new();
        let texts = vec!["alpha".to_string(), "beta".to_string()];

        // First call — both miss; fetch invoked with both inputs.
        let mut fetch_calls = 0;
        let res1 = embed_with_cache(&cache, 100, &texts, |missing| {
            fetch_calls += 1;
            assert_eq!(missing.len(), 2);
            Ok(missing.iter().map(|t| canned_vec(t)).collect())
        })
        .unwrap();
        assert_eq!(fetch_calls, 1);
        assert_eq!(res1.len(), 2);
        assert_eq!(cache.len(), 2);

        // Second call with same inputs — both hit; fetch never invoked.
        let res2 = embed_with_cache(&cache, 100, &texts, |_| {
            panic!("fetch must not be called when every input is cached");
        })
        .unwrap();
        assert_eq!(res2, res1);
    }

    #[test]
    fn partial_cache_hit_only_fetches_misses() {
        let cache: DashMap<u64, Vec<f32>> = DashMap::new();
        cache.insert(xxh3_64(b"alpha"), canned_vec("alpha"));

        let texts = vec!["alpha".to_string(), "gamma".to_string()];
        let res = embed_with_cache(&cache, 100, &texts, |missing| {
            assert_eq!(missing, &vec!["gamma".to_string()]);
            Ok(vec![canned_vec("gamma")])
        })
        .unwrap();
        assert_eq!(res[0], canned_vec("alpha"));
        assert_eq!(res[1], canned_vec("gamma"));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn cache_clears_when_over_budget() {
        let cache: DashMap<u64, Vec<f32>> = DashMap::new();
        for i in 0..3u8 {
            cache.insert(u64::from(i), vec![1.0]);
        }
        // budget = 3, fetch adds 1 more → cache flushed before insert,
        // ending state is just the freshly-fetched entry.
        let texts = vec!["new".to_string()];
        embed_with_cache(&cache, 3, &texts, |missing| {
            Ok(missing.iter().map(|t| canned_vec(t)).collect())
        })
        .unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key(&xxh3_64(b"new")));
    }

    #[test]
    fn zero_capacity_disables_cache() {
        let cache: DashMap<u64, Vec<f32>> = DashMap::new();
        let texts = vec!["x".to_string()];
        let mut calls = 0;
        for _ in 0..3 {
            embed_with_cache(&cache, 0, &texts, |missing| {
                calls += 1;
                Ok(missing.iter().map(|t| canned_vec(t)).collect())
            })
            .unwrap();
        }
        assert_eq!(calls, 3, "every call should hit fetch when cache is disabled");
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn empty_input_returns_empty_without_fetch() {
        let cache: DashMap<u64, Vec<f32>> = DashMap::new();
        let res = embed_with_cache(&cache, 100, &[], |_| {
            panic!("fetch must not be called for empty input");
        })
        .unwrap();
        assert!(res.is_empty());
    }

    #[test]
    fn map_model_known_aliases() {
        assert!(map_model("bge-small-en-v1.5-int8").is_ok());
        assert!(map_model("BGE-Small-EN-v1.5-INT8").is_ok());
        assert!(map_model("").is_ok(), "empty falls back to default");
        assert!(map_model("mxbai-embed-large-v1").is_ok());
    }

    #[test]
    fn map_model_unknown_returns_provider_error() {
        let err = map_model("definitely-not-a-model").unwrap_err();
        assert!(matches!(err, AiError::Provider(_)));
    }

    #[test]
    fn fetch_count_mismatch_is_provider_error() {
        let cache: DashMap<u64, Vec<f32>> = DashMap::new();
        let texts = vec!["a".to_string(), "b".to_string()];
        let err = embed_with_cache(&cache, 100, &texts, |_| Ok(vec![vec![1.0]])).unwrap_err();
        assert!(matches!(err, AiError::Provider(_)));
    }

    /// End-to-end test that loads the real fastembed model. Ignored by
    /// default because it downloads ~33 MB from `HuggingFace` on first
    /// run. Run with:
    ///
    /// ```text
    /// cargo test -p nexus-ai --features local-embeddings -- --ignored
    /// ```
    #[test]
    #[ignore = "downloads ~33MB from HuggingFace on first run"]
    fn local_embedding_round_trip() {
        let backend = LocalEmbedding::new("bge-small-en-v1.5-int8").unwrap();
        assert_eq!(backend.dimension(), 384);

        let texts = vec!["hello world".to_string(), "foo bar baz".to_string()];
        let v1 = backend.embed_batch(&texts).unwrap();
        assert_eq!(v1.len(), 2);
        assert_eq!(v1[0].len(), 384);

        // Second call hits the cache.
        let before = backend.cache_len();
        let v2 = backend.embed_batch(&texts).unwrap();
        assert_eq!(v2, v1);
        assert_eq!(backend.cache_len(), before);
    }
}
