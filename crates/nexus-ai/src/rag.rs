//! Retrieval-Augmented Generation (RAG) pipeline.
//!
//! Combines HTTP-based embedding + chat providers with storage-owned vector
//! search (reached through `com.nexus.storage` IPC). The pipeline does not
//! touch `SQLite` directly.

use std::fmt::Write as _;

use nexus_kernel::KernelPluginContext;
use serde::{Deserialize, Serialize};

use crate::chunker::chunks_from_blocks;
use crate::embedding::EmbeddingProvider;
use crate::error::AiError;
use crate::privacy::Redactor;
use crate::provider::{AiProvider, ChatMessage, Role};
use crate::tokens::{BudgetWarning, ContextSourceKind, TokenBudget, TokenCounter};
use crate::vectorstore::{self, ChunkEmbedding, ChunkMatch};

/// Default maximum chunk size in characters.
const DEFAULT_MAX_CHUNK_SIZE: usize = 1024;

/// The response from a RAG query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagResponse {
    /// Generated answer text.
    pub answer: String,
    /// Source chunks retrieved to ground the answer.
    pub sources: Vec<ChunkMatch>,
    /// Name of the model that generated the answer.
    pub model: String,
}

/// Answer a question using retrieval-augmented generation.
///
/// Embeds the question, fetches the top `limit` matching chunks via storage
/// IPC, builds a grounded system prompt, and calls the AI provider.
///
/// # Errors
/// Returns [`AiError`] if embedding, vector search, or the chat call fails.
pub async fn query(
    ctx: &KernelPluginContext,
    ai: &dyn AiProvider,
    embedder: &dyn EmbeddingProvider,
    question: &str,
    limit: usize,
) -> Result<RagResponse, AiError> {
    let q_embeddings = embedder.embed(&[question.to_string()]).await?;
    let q_embedding = q_embeddings
        .into_iter()
        .next()
        .ok_or_else(|| AiError::Provider("embedding returned no vectors".into()))?;

    let sources = vectorstore::search(ctx, &q_embedding, limit).await?;
    let system = build_rag_prompt(&sources);

    let messages = vec![ChatMessage {
        role: Role::User,
        content: question.to_string(),
    }];

    let answer = ai.chat(&messages, Some(&system)).await?;

    Ok(RagResponse {
        answer,
        sources,
        model: ai.model_name().to_string(),
    })
}

/// Index a file's blocks by chunking, embedding, and upserting via storage
/// IPC. Returns the number of chunks stored.
///
/// # Errors
/// Returns [`AiError`] if embedding or the storage call fails.
pub async fn index_file(
    ctx: &KernelPluginContext,
    embedder: &dyn EmbeddingProvider,
    file_path: &str,
    blocks: &[(u64, String, String, Option<i32>)],
) -> Result<usize, AiError> {
    let chunks = chunks_from_blocks(file_path, blocks, DEFAULT_MAX_CHUNK_SIZE);

    if chunks.is_empty() {
        vectorstore::delete_by_file(ctx, file_path).await?;
        return Ok(0);
    }

    let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let embeddings = embedder.embed(&texts).await?;

    let chunk_embeddings: Vec<ChunkEmbedding> = chunks
        .into_iter()
        .zip(embeddings)
        .map(|(chunk, emb)| ChunkEmbedding {
            file_path: chunk.file_path,
            block_id: chunk.block_id,
            chunk_text: chunk.content,
            embedding: emb,
        })
        .collect();

    let n = chunk_embeddings.len();
    vectorstore::upsert(ctx, file_path, &chunk_embeddings).await?;
    Ok(n)
}

/// Embed the question and retrieve the top-`limit` chunks from the
/// vector store. Callers that want to reuse the prompt-assembly half
/// of [`query`] without the blocking chat step — e.g. a streaming
/// RAG handler — combine this with [`build_rag_prompt`] and drive
/// their own provider.
///
/// # Errors
/// Returns [`AiError`] if embedding or vector search fails.
pub async fn retrieve(
    ctx: &KernelPluginContext,
    embedder: &dyn EmbeddingProvider,
    question: &str,
    limit: usize,
) -> Result<Vec<ChunkMatch>, AiError> {
    let q_embeddings = embedder.embed(&[question.to_string()]).await?;
    let q_embedding = q_embeddings
        .into_iter()
        .next()
        .ok_or_else(|| AiError::Provider("embedding returned no vectors".into()))?;
    vectorstore::search(ctx, &q_embedding, limit).await
}

/// BL-040: Embed `query` and return the top-`limit` matching chunks
/// from the vector store, without invoking any chat provider.
///
/// This is the retrieval half of [`query`] exposed as a standalone
/// surface so palette / TUI / MCP callers can do "search by meaning"
/// without paying for a chat round-trip. Implementation-wise this is
/// a thin alias for [`retrieve`] kept under a name that matches the
/// IPC handler so it's easy to grep.
///
/// # Errors
/// Returns [`AiError`] if embedding or vector search fails.
pub async fn semantic_search(
    ctx: &KernelPluginContext,
    embedder: &dyn EmbeddingProvider,
    query: &str,
    limit: usize,
) -> Result<Vec<ChunkMatch>, AiError> {
    retrieve(ctx, embedder, query, limit).await
}

/// Default fallback system prompt used when no RAG sources are available.
const RAG_FALLBACK_PROMPT: &str =
    "You are a helpful assistant. Answer the user's question to the best of your ability.";

/// Header prefixed onto a prompt that includes RAG context.
const RAG_PROMPT_HEADER: &str =
    "Use the following context from the user's notes to answer their question. \
     Cite sources using [[file_path]] notation when relevant.\n\n";

/// Utilisation threshold (`used / available`) at which
/// [`build_rag_prompt_budgeted`] emits a [`BudgetWarning::NearLimit`].
const NEAR_LIMIT_THRESHOLD: f32 = 0.80;

/// Build the system prompt for the RAG conversation.
///
/// Thin wrapper over [`build_rag_prompt_budgeted`] with an effectively
/// unlimited budget and no redactor — every source is always included
/// verbatim and no warnings are surfaced. Streaming callers that want
/// to enforce a model-aware context window or redact secrets before
/// egress should call [`build_rag_prompt_budgeted`] directly.
///
/// BL-018 contract: this wrapper's byte output must remain identical
/// to its pre-BL-017 behaviour, hence the `None` redactor.
#[must_use]
pub fn build_rag_prompt(sources: &[ChunkMatch]) -> String {
    let mut budget = TokenBudget::new(usize::MAX, 0);
    let counter = crate::tokens::ApproxTokenCounter;
    let (prompt, _warnings) = build_rag_prompt_budgeted(sources, &mut budget, &counter, None);
    prompt
}

/// Assemble the RAG system prompt while respecting `budget`.
///
/// Sources are considered in descending `score` order (highest scoring
/// first). Each source is charged against the budget under
/// [`ContextSourceKind::RagChunk`]; sources that don't fit are dropped
/// and surface as [`BudgetWarning::SourceDropped`]. The final prompt
/// contains only the sources that were actually allocated.
///
/// Emits [`BudgetWarning::NearLimit`] when realised utilisation is
/// `>= 0.80` after assembly so callers can warn the user that the
/// context window is nearly exhausted.
///
/// When `sources` is empty the legacy fallback prompt is returned and
/// no warnings are produced — matching [`build_rag_prompt`]'s behaviour.
///
/// When `redactor` is `Some(_)`, every accepted chunk's `chunk_text`
/// is run through [`Redactor::redact_in_place`] before it's appended
/// to the prompt body — the redacted bytes are what the budget pays
/// for and what the model sees. Passing `None` preserves the
/// pre-BL-017 byte-for-byte output (this is what
/// [`build_rag_prompt`] does).
pub fn build_rag_prompt_budgeted(
    sources: &[ChunkMatch],
    budget: &mut TokenBudget,
    counter: &dyn TokenCounter,
    redactor: Option<&Redactor>,
) -> (String, Vec<BudgetWarning>) {
    if sources.is_empty() {
        return (RAG_FALLBACK_PROMPT.to_string(), Vec::new());
    }

    // Sort by descending score so the most relevant chunks get first
    // shot at the budget; ties preserve original order (stable sort).
    let mut ordered: Vec<&ChunkMatch> = sources.iter().collect();
    ordered.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut warnings: Vec<BudgetWarning> = Vec::new();
    // Materialise an owned `chunk_text` per source so the redactor (if
    // any) can rewrite the string before we charge it against the
    // budget. Without redaction this is one allocation per source —
    // not measurable next to the existing prompt assembly cost.
    let mut accepted: Vec<(String, String)> = Vec::new();

    for source in ordered {
        let mut text = source.chunk_text.clone();
        if let Some(r) = redactor {
            // Discard the per-match Redaction events at this layer —
            // the budgeted-prompt API has no diagnostics channel for
            // them today. A future caller that wants the events can
            // call Redactor::redact directly and assemble the prompt
            // themselves.
            let _ = r.redact_in_place(&mut text);
        }
        // Cost the rendered "Source N: [[path]]\n<text>\n\n" line. The
        // index isn't known until assembly, so use a stable upper-bound
        // template: "Source 99: [[<path>]]\n<text>\n\n".
        let rendered = format!("Source 99: [[{}]]\n{}\n\n", source.file_path, text);
        let tokens = counter.count_tokens(&rendered);
        if budget.allocate(ContextSourceKind::RagChunk, tokens) {
            accepted.push((source.file_path.clone(), text));
        } else {
            warnings.push(BudgetWarning::SourceDropped {
                kind: ContextSourceKind::RagChunk,
                tokens,
            });
        }
    }

    if accepted.is_empty() {
        // Every source was dropped. Fall back to the no-context prompt
        // but keep the SourceDropped warnings so the caller sees why.
        return (RAG_FALLBACK_PROMPT.to_string(), warnings);
    }

    let mut prompt = String::from(RAG_PROMPT_HEADER);
    for (i, (file_path, text)) in accepted.iter().enumerate() {
        let _ = write!(prompt, "Source {}: [[{}]]\n{}\n\n", i + 1, file_path, text,);
    }

    if budget.utilization() >= NEAR_LIMIT_THRESHOLD {
        warnings.push(BudgetWarning::NearLimit {
            utilization: budget.utilization(),
        });
    }

    (prompt, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ApproxTokenCounter;
    use async_trait::async_trait;
    use nexus_kernel::{
        CapabilitySet, EventBus, InMemoryKvStore, IpcDispatcher, IpcError, IpcFuture,
        KernelPluginContext, KvStore,
    };
    use std::sync::{Arc, Mutex};

    /// Embedder stub: returns a fixed vector regardless of input. Records
    /// the texts it was asked to embed so tests can assert on them.
    struct StubEmbedder {
        vector: Vec<f32>,
        seen: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl EmbeddingProvider for StubEmbedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AiError> {
            self.seen.lock().unwrap().extend_from_slice(texts);
            Ok(texts.iter().map(|_| self.vector.clone()).collect())
        }
        fn dimension(&self) -> usize {
            self.vector.len()
        }
    }

    /// IPC dispatcher stub: only knows how to answer
    /// `com.nexus.storage::vector_query` with a canned `Vec<ChunkMatch>`,
    /// and records the args it was called with.
    struct StubDispatcher {
        matches: Vec<ChunkMatch>,
        seen: Mutex<Vec<(String, String, serde_json::Value)>>,
    }

    impl IpcDispatcher for StubDispatcher {
        fn dispatch(
            &self,
            target: &str,
            command: &str,
            _args: &serde_json::Value,
        ) -> Result<serde_json::Value, IpcError> {
            Err(IpcError::CommandNotFound {
                plugin_id: target.to_string(),
                command: command.to_string(),
            })
        }

        fn dispatch_async(
            &self,
            target: &str,
            command: &str,
            args: serde_json::Value,
        ) -> Option<IpcFuture> {
            self.seen
                .lock()
                .unwrap()
                .push((target.to_string(), command.to_string(), args.clone()));
            if target == "com.nexus.storage" && command == "vector_query" {
                let resp = serde_json::to_value(&self.matches).unwrap();
                Some(Box::pin(async move { Ok(resp) }))
            } else {
                None
            }
        }
    }

    fn make_ctx(dispatcher: Arc<dyn IpcDispatcher>) -> (KernelPluginContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
        let bus = Arc::new(EventBus::new(16));
        let caps: CapabilitySet = [nexus_kernel::Capability::IpcCall].into_iter().collect();
        let ctx = KernelPluginContext::new(
            "com.nexus.ai",
            "0.0.1",
            caps,
            kv,
            bus,
            dir.path(),
            Some(dispatcher),
        )
        .unwrap();
        (ctx, dir)
    }

    #[tokio::test]
    async fn semantic_search_embeds_query_and_forwards_to_storage() {
        let canned = vec![
            ChunkMatch {
                file_path: "notes/a.md".into(),
                block_id: 1,
                chunk_text: "alpha".into(),
                score: 0.9,
            },
            ChunkMatch {
                file_path: "notes/b.md".into(),
                block_id: 2,
                chunk_text: "beta".into(),
                score: 0.7,
            },
        ];
        let dispatcher = Arc::new(StubDispatcher {
            matches: canned.clone(),
            seen: Mutex::new(Vec::new()),
        });
        let embedder = StubEmbedder {
            vector: vec![0.1, 0.2, 0.3],
            seen: Mutex::new(Vec::new()),
        };
        let (ctx, _tmp) = make_ctx(dispatcher.clone());

        let out = semantic_search(&ctx, &embedder, "find me alpha", 4)
            .await
            .expect("semantic_search ok");

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].file_path, "notes/a.md");
        // Embedder saw the query exactly once.
        assert_eq!(embedder.seen.lock().unwrap().as_slice(), &["find me alpha"]);
        // Dispatcher saw a single vector_query against storage with our
        // embedding + limit.
        let seen = dispatcher.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "com.nexus.storage");
        assert_eq!(seen[0].1, "vector_query");
        assert_eq!(seen[0].2["limit"], 4);
        let emb = seen[0].2["embedding"].as_array().expect("embedding array");
        assert_eq!(emb.len(), 3);
        let nums: Vec<f64> = emb.iter().map(|v| v.as_f64().unwrap()).collect();
        for (got, want) in nums.iter().zip([0.1f64, 0.2, 0.3]) {
            assert!((got - want).abs() < 1e-5, "{got} ≉ {want}");
        }
    }

    #[tokio::test]
    async fn semantic_search_propagates_embedder_errors() {
        struct Failing;
        #[async_trait]
        impl EmbeddingProvider for Failing {
            async fn embed(&self, _: &[String]) -> Result<Vec<Vec<f32>>, AiError> {
                Err(AiError::Provider("nope".into()))
            }
            fn dimension(&self) -> usize {
                0
            }
        }
        let dispatcher = Arc::new(StubDispatcher {
            matches: Vec::new(),
            seen: Mutex::new(Vec::new()),
        });
        let (ctx, _tmp) = make_ctx(dispatcher);
        let err = semantic_search(&ctx, &Failing, "q", 5).await.unwrap_err();
        assert!(matches!(err, AiError::Provider(_)));
    }

    #[test]
    fn build_rag_prompt_with_no_sources() {
        let prompt = build_rag_prompt(&[]);
        assert!(prompt.contains("helpful assistant"));
    }

    #[test]
    fn build_rag_prompt_with_sources() {
        let sources = vec![
            ChunkMatch {
                file_path: "notes/rust.md".into(),
                block_id: 1,
                chunk_text: "Rust is a systems programming language.".into(),
                score: 0.95,
            },
            ChunkMatch {
                file_path: "notes/go.md".into(),
                block_id: 2,
                chunk_text: "Go is great for servers.".into(),
                score: 0.80,
            },
        ];
        let prompt = build_rag_prompt(&sources);
        assert!(prompt.contains("[[notes/rust.md]]"));
        assert!(prompt.contains("[[notes/go.md]]"));
        assert!(prompt.contains("Source 1"));
        assert!(prompt.contains("Source 2"));
    }

    #[test]
    fn budgeted_prompt_includes_all_sources_when_under_budget() {
        let sources = vec![
            ChunkMatch {
                file_path: "notes/rust.md".into(),
                block_id: 1,
                chunk_text: "Rust is a systems programming language.".into(),
                score: 0.95,
            },
            ChunkMatch {
                file_path: "notes/go.md".into(),
                block_id: 2,
                chunk_text: "Go is great for servers.".into(),
                score: 0.80,
            },
        ];
        let mut budget = TokenBudget::new(10_000, 1_000);
        let counter = ApproxTokenCounter;
        let (prompt, warnings) = build_rag_prompt_budgeted(&sources, &mut budget, &counter, None);

        // Same shape as the legacy prompt: header + numbered sources.
        let legacy = build_rag_prompt(&sources);
        assert_eq!(prompt, legacy);
        assert!(prompt.contains("[[notes/rust.md]]"));
        assert!(prompt.contains("[[notes/go.md]]"));
        // Plenty of headroom -> no warnings.
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn budgeted_prompt_drops_lowest_score_first_when_over_budget() {
        // Three chunks with distinct scores. Pick a budget that fits ~two
        // chunks but not three (the rendered template is roughly 30 chars
        // including overhead, so each chunk costs ~10 tokens via the
        // 4-chars-per-token approximation).
        let sources = vec![
            ChunkMatch {
                file_path: "a.md".into(),
                block_id: 1,
                chunk_text: "alpha alpha alpha alpha alpha".into(),
                score: 0.95,
            },
            ChunkMatch {
                file_path: "b.md".into(),
                block_id: 2,
                chunk_text: "beta beta beta beta beta".into(),
                score: 0.80,
            },
            ChunkMatch {
                file_path: "c.md".into(),
                block_id: 3,
                chunk_text: "gamma gamma gamma gamma gamma".into(),
                score: 0.60,
            },
        ];
        let counter = ApproxTokenCounter;
        // Render each source under the upper-bound template the assembler
        // uses so we can size the budget to fit exactly two of three.
        let costs: Vec<usize> = sources
            .iter()
            .map(|s| {
                counter.count_tokens(&format!(
                    "Source 99: [[{}]]\n{}\n\n",
                    s.file_path, s.chunk_text
                ))
            })
            .collect();
        // Sort descending by score for the pick order: 0.95, 0.80, 0.60.
        // Want sum(costs[0] + costs[1]) to fit and adding costs[2] to bust.
        let fit_two = costs[0] + costs[1];
        let mut budget = TokenBudget::new(fit_two + 5, 0);

        let (prompt, warnings) = build_rag_prompt_budgeted(&sources, &mut budget, &counter, None);

        assert!(prompt.contains("[[a.md]]"));
        assert!(prompt.contains("[[b.md]]"));
        assert!(
            !prompt.contains("[[c.md]]"),
            "lowest-scoring source should have been dropped: {prompt}"
        );
        // Exactly one SourceDropped warning, for the 0.60-scored chunk.
        let dropped: Vec<&BudgetWarning> = warnings
            .iter()
            .filter(|w| matches!(w, BudgetWarning::SourceDropped { .. }))
            .collect();
        assert_eq!(dropped.len(), 1, "warnings: {warnings:?}");
    }

    #[test]
    fn budgeted_prompt_emits_near_limit_warning_at_80pct() {
        let sources = vec![ChunkMatch {
            file_path: "n.md".into(),
            block_id: 1,
            chunk_text: "lorem ipsum dolor sit amet consectetur".into(),
            score: 0.9,
        }];
        let counter = ApproxTokenCounter;
        let cost = counter.count_tokens(&format!(
            "Source 99: [[{}]]\n{}\n\n",
            sources[0].file_path, sources[0].chunk_text
        ));
        // Available budget = cost itself => utilisation = 1.0 ≥ 0.80.
        let mut budget = TokenBudget::new(cost, 0);
        let (_prompt, warnings) = build_rag_prompt_budgeted(&sources, &mut budget, &counter, None);
        let near = warnings
            .iter()
            .find(|w| matches!(w, BudgetWarning::NearLimit { .. }));
        assert!(
            near.is_some(),
            "expected NearLimit warning, got: {warnings:?}"
        );
        if let Some(BudgetWarning::NearLimit { utilization }) = near {
            assert!(
                *utilization >= 0.80,
                "utilization should be >= 0.80, was {utilization}"
            );
        }
    }

    #[test]
    fn budgeted_prompt_with_zero_sources_returns_legacy_default() {
        let counter = ApproxTokenCounter;
        let mut budget = TokenBudget::new(1_000, 100);
        let (prompt, warnings) = build_rag_prompt_budgeted(&[], &mut budget, &counter, None);
        assert_eq!(prompt, build_rag_prompt(&[]));
        assert!(prompt.contains("helpful assistant"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn budgeted_prompt_redacts_chunk_text_when_redactor_supplied() {
        let sources = vec![ChunkMatch {
            file_path: "secrets.md".into(),
            block_id: 1,
            chunk_text: "deploy key=AKIAIOSFODNN7EXAMPLE rest of note".into(),
            score: 0.9,
        }];
        let counter = ApproxTokenCounter;
        let mut budget = TokenBudget::new(10_000, 0);
        let redactor = crate::privacy::Redactor::with_default_patterns();
        let (prompt, _warnings) =
            build_rag_prompt_budgeted(&sources, &mut budget, &counter, Some(&redactor));
        assert!(
            prompt.contains("[REDACTED:aws-access-key]"),
            "expected redaction placeholder in prompt: {prompt}"
        );
        assert!(
            !prompt.contains("AKIAIOSFODNN7EXAMPLE"),
            "raw secret leaked into prompt: {prompt}"
        );
        // Source framing still intact.
        assert!(prompt.contains("[[secrets.md]]"));
        assert!(prompt.contains("Source 1"));
    }

    #[test]
    fn budgeted_prompt_passes_through_when_redactor_is_none() {
        // BL-018 contract: with `None` redactor the output must remain
        // byte-identical to the legacy `build_rag_prompt` wrapper, so a
        // chunk that *contains* a secret-shaped string still passes
        // through verbatim. We use an AWS-looking string to make the
        // pass-through unambiguous.
        let sources = vec![
            ChunkMatch {
                file_path: "a.md".into(),
                block_id: 1,
                chunk_text: "value=AKIAIOSFODNN7EXAMPLE".into(),
                score: 0.9,
            },
            ChunkMatch {
                file_path: "b.md".into(),
                block_id: 2,
                chunk_text: "ordinary content".into(),
                score: 0.5,
            },
        ];
        let counter = ApproxTokenCounter;
        let mut budget = TokenBudget::new(10_000, 0);
        let (prompt, _warnings) = build_rag_prompt_budgeted(&sources, &mut budget, &counter, None);
        let legacy = build_rag_prompt(&sources);
        assert_eq!(
            prompt, legacy,
            "BL-018 byte-identity broken when redactor is None"
        );
        // And the secret really is intact in both.
        assert!(prompt.contains("AKIAIOSFODNN7EXAMPLE"));
    }
}
