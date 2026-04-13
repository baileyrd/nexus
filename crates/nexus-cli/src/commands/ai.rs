//! AI command handlers: ask, embed, status, config.

use anyhow::Result;
use crate::app::App;

/// Ask a question using RAG against the forge.
pub fn ask(app: &mut App, question: &str) -> Result<()> {
    let storage = app.storage_mut()?;

    let ai_config = nexus_ai::detect_provider()
        .ok_or_else(|| anyhow::anyhow!("{}", nexus_ai::AiError::NoProvider))?;
    let embed_config = nexus_ai::detect_embedding_provider()
        .ok_or_else(|| anyhow::anyhow!("{}", nexus_ai::AiError::NoEmbeddingProvider))?;

    let ai: Box<dyn nexus_ai::AiProvider> = build_ai_provider(&ai_config)?;
    let embedder: Box<dyn nexus_ai::EmbeddingProvider> = build_embedding_provider(&embed_config)?;

    let conn = storage.pool_connection()
        .map_err(|e| anyhow::anyhow!("failed to get DB connection: {e}"))?;

    let rt = tokio::runtime::Runtime::new()?;
    let response = rt.block_on(nexus_ai::rag_query(
        &conn, ai.as_ref(), embedder.as_ref(), question, 5,
    ))
    .map_err(|e| anyhow::anyhow!("AI query failed: {e}"))?;

    println!("{}", response.answer);

    if !response.sources.is_empty() {
        println!("\n--- Sources ---");
        for source in &response.sources {
            println!("  [{:.2}] {}", source.score, source.file_path);
        }
    }

    Ok(())
}

/// Generate embeddings for all files or a single file.
pub fn embed(app: &mut App, file: Option<&str>) -> Result<()> {
    let storage = app.storage_mut()?;

    let embed_config = nexus_ai::detect_embedding_provider()
        .ok_or_else(|| anyhow::anyhow!("{}", nexus_ai::AiError::NoEmbeddingProvider))?;
    let embedder: Box<dyn nexus_ai::EmbeddingProvider> = build_embedding_provider(&embed_config)?;

    let conn = storage.pool_connection()
        .map_err(|e| anyhow::anyhow!("failed to get DB connection: {e}"))?;

    let rt = tokio::runtime::Runtime::new()?;

    if let Some(path) = file {
        let file_record = nexus_storage::file_by_path(&conn, path)?
            .ok_or_else(|| anyhow::anyhow!("file not found: {path}"))?;
        let blocks = nexus_storage::query_blocks(&conn, file_record.id)?;
        let block_tuples: Vec<_> = blocks.iter().map(|b| {
            (b.id, b.block_type.clone(), b.content.clone(), b.level)
        }).collect();

        let count = rt.block_on(nexus_ai::rag_index_file(
            &conn, embedder.as_ref(), path, &block_tuples,
        ))
        .map_err(|e| anyhow::anyhow!("embedding failed: {e}"))?;

        println!("Embedded {count} chunks from {path}");
    } else {
        let files = nexus_storage::query_files(&conn, &nexus_storage::FileFilter::default())?;
        let mut total = 0;

        for file_record in &files {
            let blocks = nexus_storage::query_blocks(&conn, file_record.id)?;
            let block_tuples: Vec<_> = blocks.iter().map(|b| {
                (b.id, b.block_type.clone(), b.content.clone(), b.level)
            }).collect();

            let count = rt.block_on(nexus_ai::rag_index_file(
                &conn, embedder.as_ref(), &file_record.path, &block_tuples,
            ))
            .map_err(|e| anyhow::anyhow!("embedding failed for {}: {e}", file_record.path))?;

            total += count;
            println!("  {} — {count} chunks", file_record.path);
        }

        println!("\nEmbedded {total} chunks from {} files", files.len());
    }

    Ok(())
}

/// Show AI and embedding status.
pub fn status(app: &mut App) -> Result<()> {
    let storage = app.storage()?;
    let conn = storage.pool_connection()
        .map_err(|e| anyhow::anyhow!("failed to get DB connection: {e}"))?;

    let embedding_count = nexus_ai::vectorstore_count(&conn)
        .map_err(|e| anyhow::anyhow!("failed to count embeddings: {e}"))?;

    let ai_provider = nexus_ai::detect_provider()
        .map(|c| format!("{} ({})", c.provider, c.model.unwrap_or_else(|| "default".to_string())))
        .unwrap_or_else(|| "none".to_string());

    let embed_provider = nexus_ai::detect_embedding_provider()
        .map(|c| c.provider)
        .unwrap_or_else(|| "none".to_string());

    println!("AI Provider       : {ai_provider}");
    println!("Embedding Provider: {embed_provider}");
    println!("Indexed Chunks    : {embedding_count}");

    Ok(())
}

/// Show current AI configuration.
pub fn config() -> Result<()> {
    let ai = nexus_ai::detect_provider();
    let embed = nexus_ai::detect_embedding_provider();

    println!("--- AI Provider ---");
    match ai {
        Some(c) => {
            println!("Provider : {}", c.provider);
            println!("Model    : {}", c.model.unwrap_or_else(|| "default".to_string()));
            println!("API Key  : {}", if c.api_key.is_some() { "set" } else { "not set" });
            if let Some(url) = c.base_url {
                println!("Base URL : {url}");
            }
        }
        None => println!("Not configured"),
    }

    println!("\n--- Embedding Provider ---");
    match embed {
        Some(c) => {
            println!("Provider : {}", c.provider);
            println!("API Key  : {}", if c.api_key.is_some() { "set" } else { "not set" });
        }
        None => println!("Not configured"),
    }

    Ok(())
}

// -- Private helpers --

/// Build an AI provider from the detected configuration.
fn build_ai_provider(config: &nexus_ai::AiConfig) -> Result<Box<dyn nexus_ai::AiProvider>> {
    match config.provider.as_str() {
        "anthropic" => Ok(Box::new(nexus_ai::AnthropicProvider::new(
            config.api_key.clone().unwrap_or_default(),
            config.model.clone(),
            config.max_tokens,
        ))),
        "openai" => Ok(Box::new(nexus_ai::OpenAiProvider::new(
            config.api_key.clone().unwrap_or_default(),
            config.model.clone(),
            config.max_tokens,
        ))),
        "ollama" => Ok(Box::new(nexus_ai::OllamaProvider::new(
            config.base_url.clone(),
            config.model.clone(),
        ))),
        other => Err(anyhow::anyhow!("unknown AI provider: {other}")),
    }
}

/// Build an embedding provider from the detected configuration.
fn build_embedding_provider(config: &nexus_ai::AiConfig) -> Result<Box<dyn nexus_ai::EmbeddingProvider>> {
    match config.provider.as_str() {
        "openai" => Ok(Box::new(nexus_ai::OpenAiProvider::new(
            config.api_key.clone().unwrap_or_default(),
            None,
            4096,
        ))),
        "ollama" => Ok(Box::new(nexus_ai::OllamaProvider::new(
            config.base_url.clone(),
            None,
        ))),
        other => Err(anyhow::anyhow!("unknown embedding provider: {other}")),
    }
}
