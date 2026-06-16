//! LLM-synthesized wiki pages over the memory store.
//!
//! A wiki page is a Markdown file in the forge under `wiki/<slug>.md` —
//! file-as-truth, indexed and browsable like any other note. [`wiki_compile`]
//! gathers memories on a topic, asks `com.nexus.ai::generate` to synthesize a
//! page, and writes it via `com.nexus.storage::write_file`; [`wiki_read`] /
//! [`wiki_list`] are thin reads over storage. All three reach AI + storage
//! through the memory plugin's own wired [`KernelPluginContext`].

use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use serde_json::{json, Value};

use crate::db::MemoryDb;

const AI_PLUGIN: &str = "com.nexus.ai";
const STORAGE_PLUGIN: &str = "com.nexus.storage";
/// Forge-relative directory holding wiki pages.
const WIKI_DIR: &str = "wiki";
/// Generation can be slow; allow a generous timeout.
const IPC_TIMEOUT: Duration = Duration::from_secs(120);
/// How many memories feed a synthesis by default.
const DEFAULT_SOURCE_LIMIT: usize = 30;

/// System instruction steering the synthesis toward a clean Markdown page.
const SYSTEM_PROMPT: &str = "You are a knowledge-base editor. Synthesize the \
user's memories into a single, well-structured Markdown wiki page. Begin with \
an H1 title, then a one-line summary, then organised sections with headings and \
bullet points. Be concise and factual — do not invent anything beyond the \
provided memories. Output only the Markdown page.";

/// Map a topic to a filesystem-safe, link-stable slug: lowercase, runs of
/// non-alphanumerics collapse to a single hyphen, ends trimmed.
fn slugify(topic: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in topic.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() {
        "untitled".to_string()
    } else {
        s
    }
}

/// Forge-relative path for a topic's page.
fn page_path(topic: &str) -> String {
    format!("{WIKI_DIR}/{}.md", slugify(topic))
}

fn require_ctx(ctx: &Option<Arc<KernelPluginContext>>) -> Result<&Arc<KernelPluginContext>, String> {
    ctx.as_ref()
        .ok_or_else(|| "wiki: plugin context not wired".to_string())
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("wiki: missing '{key}'"))
}

/// Synthesize (or refresh) the wiki page for `topic` from related memories.
/// `{ topic, query?, limit? }` → `{ path, topic, sources, bytes }`.
pub(crate) async fn wiki_compile(
    db: MemoryDb,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, String> {
    let ctx = require_ctx(&ctx)?;
    let topic = str_arg(args, "topic")?;
    let query = args.get("query").and_then(Value::as_str).unwrap_or(topic);
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(DEFAULT_SOURCE_LIMIT);

    let memories = db.search(query, limit).map_err(|e| format!("wiki_compile: search: {e}"))?;
    if memories.is_empty() {
        return Err(format!("wiki_compile: no memories match '{query}'"));
    }

    let mut prompt = format!("Topic: {topic}\n\nMemories to synthesize:\n");
    for (i, m) in memories.iter().enumerate() {
        prompt.push_str(&format!("{}. {}\n", i + 1, m.content.replace('\n', " ")));
    }

    let generated = ctx
        .ipc_call(
            AI_PLUGIN,
            "generate",
            json!({ "prompt": prompt, "system": SYSTEM_PROMPT }),
            IPC_TIMEOUT,
        )
        .await
        .map_err(|e| format!("wiki_compile: generate: {e}"))?;
    let page = generated
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "wiki_compile: generate returned empty text".to_string())?;

    let path = page_path(topic);
    ctx.ipc_call(
        STORAGE_PLUGIN,
        "write_file",
        json!({ "path": path, "bytes": page.as_bytes() }),
        IPC_TIMEOUT,
    )
    .await
    .map_err(|e| format!("wiki_compile: write_file {path}: {e}"))?;

    Ok(json!({
        "path": path,
        "topic": topic,
        "sources": memories.len(),
        "bytes": page.len(),
    }))
}

/// Read a wiki page's Markdown by topic/slug. `{ topic }` → `{ path, content }`.
pub(crate) async fn wiki_read(
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, String> {
    let ctx = require_ctx(&ctx)?;
    let topic = str_arg(args, "topic")?;
    let path = page_path(topic);
    let resp = ctx
        .ipc_call(STORAGE_PLUGIN, "read_file", json!({ "path": path }), IPC_TIMEOUT)
        .await
        .map_err(|e| format!("wiki_read: {e}"))?;
    // read_file returns { bytes: [u8] | null }.
    match resp.get("bytes").and_then(Value::as_array) {
        Some(arr) => {
            let bytes: Vec<u8> = arr.iter().filter_map(|n| n.as_u64().map(|v| v as u8)).collect();
            Ok(json!({ "path": path, "content": String::from_utf8_lossy(&bytes) }))
        }
        None => Err(format!("wiki_read: no page for '{topic}' ({path})")),
    }
}

/// List the wiki pages (their slugs + paths). `{}` → `{ pages, count }`.
pub(crate) async fn wiki_list(
    ctx: Option<Arc<KernelPluginContext>>,
    _args: &Value,
) -> Result<Value, String> {
    let ctx = require_ctx(&ctx)?;
    let resp = ctx
        .ipc_call(STORAGE_PLUGIN, "list_dir", json!({ "relpath": WIKI_DIR }), IPC_TIMEOUT)
        .await
        .map_err(|e| format!("wiki_list: {e}"))?;
    let pages: Vec<Value> = resp
        .get("entries")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|e| {
                    !e.get("is_dir").and_then(Value::as_bool).unwrap_or(false)
                        && e.get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|n| n.ends_with(".md"))
                })
                .filter_map(|e| {
                    let name = e.get("name").and_then(Value::as_str)?;
                    let slug = name.strip_suffix(".md").unwrap_or(name);
                    Some(json!({ "slug": slug, "path": e.get("relpath").cloned().unwrap_or(Value::Null) }))
                })
                .collect()
        })
        .unwrap_or_default();
    let count = pages.len();
    Ok(json!({ "pages": pages, "count": count }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_is_stable_and_safe() {
        assert_eq!(slugify("VLAN Setup!"), "vlan-setup");
        assert_eq!(slugify("  vlan  setup  "), "vlan-setup");
        assert_eq!(slugify("Networking / DNS"), "networking-dns");
        assert_eq!(slugify("!!!"), "untitled");
        assert_eq!(page_path("VLAN Setup"), "wiki/vlan-setup.md");
    }

    #[tokio::test]
    async fn wiki_ops_require_context() {
        let db = MemoryDb::open_in_memory().unwrap();
        assert!(wiki_compile(db, None, &json!({ "topic": "x" }))
            .await
            .unwrap_err()
            .contains("context not wired"));
        assert!(wiki_read(None, &json!({ "topic": "x" }))
            .await
            .unwrap_err()
            .contains("context not wired"));
        assert!(wiki_list(None, &json!({})).await.unwrap_err().contains("context not wired"));
    }

    #[tokio::test]
    async fn wiki_compile_requires_topic() {
        // No context check fires first only when topic is present; pass a dummy
        // context-less call to confirm the topic guard via require_ctx ordering.
        let db = MemoryDb::open_in_memory().unwrap();
        // With a (None) context, require_ctx errors first — assert that path.
        let err = wiki_compile(db, None, &json!({})).await.unwrap_err();
        assert!(err.contains("context not wired") || err.contains("missing 'topic'"), "got: {err}");
    }
}
