//! AI command handlers — `nexus ai ask|embed|status|config`.
//!
//! Every AI call routes through `com.nexus.ai` via `ipc_call`; the CLI
//! does not link against `nexus-ai` directly.

use std::time::Duration;

use anyhow::{Context, Result};
use nexus_bootstrap::storage as storage_ipc;
use nexus_kernel::PluginContext;
use serde_json::Value;

use crate::app::App;

const AI_PLUGIN: &str = "com.nexus.ai";
const IPC_TIMEOUT: Duration = Duration::from_secs(120);

/// Ask a question using RAG.
pub fn ask(app: &mut App, question: &str) -> Result<()> {
    let response = call(app, "ask", serde_json::json!({ "question": question, "limit": 5 }))?;

    if let Some(answer) = response.get("answer").and_then(Value::as_str) {
        println!("{answer}");
    }

    if let Some(sources) = response.get("sources").and_then(Value::as_array) {
        if !sources.is_empty() {
            println!("\n--- Sources ---");
            for src in sources {
                let score = src.get("score").and_then(Value::as_f64).unwrap_or(0.0);
                let path = src
                    .get("file_path")
                    .and_then(Value::as_str)
                    .unwrap_or("<unknown>");
                println!("  [{score:.2}] {path}");
            }
        }
    }

    Ok(())
}

/// Index one file or all files into the vector store.
pub fn embed(app: &mut App, file: Option<&str>) -> Result<()> {
    if let Some(path) = file {
        let blocks = fetch_blocks(app, path)?;
        if blocks.is_empty() {
            return Err(anyhow::anyhow!("file not found: {path}"));
        }
        let count = index_one(app, path, &blocks)?;
        println!("Embedded {count} chunks from {path}");
    } else {
        let files = {
            let (runtime, rt) = app.runtime()?;
            storage_ipc::query_files(runtime, rt)?
        };

        let mut total = 0usize;
        for file_record in &files {
            let blocks = fetch_blocks(app, &file_record.path)?;
            let count = index_one(app, &file_record.path, &blocks)?;
            total += count;
            println!("  {} — {count} chunks", file_record.path);
        }
        println!("\nEmbedded {total} chunks from {} files", files.len());
    }

    Ok(())
}

fn fetch_blocks(app: &mut App, path: &str) -> Result<Vec<(u64, String, String, Option<i32>)>> {
    let (runtime, rt) = app.runtime()?;
    let blocks = storage_ipc::query_blocks(runtime, rt, path)?;
    Ok(blocks
        .into_iter()
        .map(|b| (b.id, b.block_type, b.content, b.level))
        .collect())
}

/// Show AI and embedding status + indexed-chunk count.
pub fn status(app: &mut App) -> Result<()> {
    let response = call(app, "status", serde_json::json!({}))?;

    let ai_provider = response
        .get("ai_provider")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let ai_model = response
        .get("ai_model")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let embed_provider = response
        .get("embedding_provider")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let indexed = response
        .get("indexed_chunks")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    println!("AI Provider       : {ai_provider} ({ai_model})");
    println!("Embedding Provider: {embed_provider}");
    println!("Indexed Chunks    : {indexed}");

    Ok(())
}

/// Show current AI configuration (no network calls).
pub fn config(app: &mut App) -> Result<()> {
    let response = call(app, "config", serde_json::json!({}))?;

    let print_section = |title: &str, view: Option<&Value>| {
        println!("--- {title} ---");
        match view {
            Some(Value::Object(_)) => {
                let provider = view
                    .and_then(|v| v.get("provider"))
                    .and_then(Value::as_str)
                    .unwrap_or("none");
                let model = view
                    .and_then(|v| v.get("model"))
                    .and_then(Value::as_str)
                    .unwrap_or("default");
                let has_key = view
                    .and_then(|v| v.get("has_api_key"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let base_url = view.and_then(|v| v.get("base_url")).and_then(Value::as_str);
                println!("Provider : {provider}");
                println!("Model    : {model}");
                println!("API Key  : {}", if has_key { "set" } else { "not set" });
                if let Some(url) = base_url {
                    println!("Base URL : {url}");
                }
            }
            _ => println!("Not configured"),
        }
    };

    print_section("AI Provider", response.get("ai"));
    println!();
    print_section("Embedding Provider", response.get("embedding"));

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (runtime, rt) = app.runtime()?;
    rt.block_on(
        runtime
            .context
            .ipc_call(AI_PLUGIN, command, args, IPC_TIMEOUT),
    )
    .with_context(|| format!("AI ipc call '{command}' failed"))
}

fn index_one(
    app: &mut App,
    file_path: &str,
    blocks: &[(u64, String, String, Option<i32>)],
) -> Result<usize> {
    let response = call(
        app,
        "index_file",
        serde_json::json!({ "file_path": file_path, "blocks": blocks }),
    )?;
    response
        .get("indexed_chunks")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| anyhow::anyhow!("index_file: missing 'indexed_chunks'"))
}
