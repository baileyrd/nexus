//! Explicit capture pipeline: store a conversation turn (or any text) as a
//! memory, optionally decomposing it into atomic child facts via the LLM.
//!
//! This complements the passive bus-capture in [`crate::capture`]: `auto_capture`
//! is a deliberate "remember this turn" call. The verbatim text is stored as a
//! parent memory tagged with a fresh `capture_id`; when `decompose` is set, the
//! parent is sent to `com.nexus.ai::generate`, which extracts a list of atomic
//! facts — each stored as a child memory linked by `source_capture_id`.
//! [`crate::db::MemoryDb::list_by_capture`] reassembles the lineage.

use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use serde_json::{json, Value};

use crate::db::MemoryDb;
use crate::model::{Memory, MemoryType};

const AI_PLUGIN: &str = "com.nexus.ai";
const IPC_TIMEOUT: Duration = Duration::from_secs(120);
/// Cap on facts stored from one decomposition.
const MAX_FACTS: usize = 50;

const DECOMPOSE_SYSTEM: &str = "You extract durable, atomic facts worth \
remembering from a conversation or note: preferences, decisions, action items, \
and stable knowledge. Reply with ONLY a JSON array of short, self-contained \
strings — no prose, no markdown. Omit pleasantries and anything ephemeral.";

/// Tolerantly parse the model's reply into a list of fact strings. Prefers a
/// JSON array (optionally fenced); falls back to splitting lines and stripping
/// bullet/number markers so a non-JSON reply still yields usable facts.
fn parse_facts(output: &str) -> Vec<String> {
    let trimmed = output.trim();
    // Try a JSON array anywhere in the reply.
    if let (Some(start), Some(end)) = (trimmed.find('['), trimmed.rfind(']')) {
        if end > start {
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(&trimmed[start..=end]) {
                return arr
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .take(MAX_FACTS)
                    .collect();
            }
        }
    }
    // Fallback: one fact per non-empty line, minus list markers.
    trimmed
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(['-', '*', '•'])
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
                .trim()
                .trim_matches('"')
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .take(MAX_FACTS)
        .collect()
}

/// Capture `content` as a memory, optionally decomposing it into atomic facts.
/// `{ content, client?, category?, tags?, decompose? }` →
/// `{ capture_id, parent_id, children: [id], decomposed }`.
pub(crate) async fn auto_capture(
    db: MemoryDb,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, String> {
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "auto_capture: missing 'content'".to_string())?;
    let capture_id = uuid::Uuid::now_v7().to_string();

    // Parent: the verbatim turn (episodic).
    let mut parent = Memory::new(content)
        .with_source("capture")
        .with_type(MemoryType::Episodic);
    parent.capture_id = Some(capture_id.clone());
    if let Some(c) = args.get("client").and_then(Value::as_str) {
        parent.client = c.to_string();
    }
    if let Some(c) = args.get("category").and_then(Value::as_str) {
        parent.category = c.to_string();
    }
    if let Some(tags) = args.get("tags").and_then(Value::as_array) {
        parent.tags = tags.iter().filter_map(|t| t.as_str().map(String::from)).collect();
    }
    let parent_id = parent.id.to_string();
    db.insert(&parent).map_err(|e| format!("auto_capture: insert parent: {e}"))?;

    let mut children: Vec<String> = Vec::new();
    let mut decomposed = false;
    if args.get("decompose").and_then(Value::as_bool).unwrap_or(false) {
        let ctx = ctx.ok_or_else(|| "auto_capture: decompose needs a wired context".to_string())?;
        let gen = ctx
            .ipc_call(
                AI_PLUGIN,
                "generate",
                json!({ "prompt": content, "system": DECOMPOSE_SYSTEM }),
                IPC_TIMEOUT,
            )
            .await
            .map_err(|e| format!("auto_capture: generate: {e}"))?;
        let text = gen.get("text").and_then(Value::as_str).unwrap_or_default();
        for fact in parse_facts(text) {
            let mut child = Memory::new(fact)
                .with_source("capture")
                .with_type(MemoryType::Semantic)
                .with_category(parent.category.clone());
            child.source_capture_id = Some(capture_id.clone());
            child.client.clone_from(&parent.client);
            if db.insert(&child).is_ok() {
                children.push(child.id.to_string());
            }
        }
        decomposed = true;
    }

    Ok(json!({
        "capture_id": capture_id,
        "parent_id": parent_id,
        "children": children,
        "decomposed": decomposed,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_facts_reads_json_array_even_when_fenced() {
        let out = "```json\n[\"likes rust\", \"prefers tabs\"]\n```";
        assert_eq!(parse_facts(out), vec!["likes rust", "prefers tabs"]);
        let bare = "[\"a\", \"b\", \"  \", \"c\"]";
        assert_eq!(parse_facts(bare), vec!["a", "b", "c"]); // blanks dropped
    }

    #[test]
    fn parse_facts_falls_back_to_lines() {
        let out = "- first fact\n- second fact\n3. third fact";
        assert_eq!(parse_facts(out), vec!["first fact", "second fact", "third fact"]);
        assert!(parse_facts("   ").is_empty());
    }

    #[tokio::test]
    async fn auto_capture_stores_parent_without_decompose() {
        let db = MemoryDb::open_in_memory().unwrap();
        let out = auto_capture(
            db.clone(),
            None,
            &json!({ "content": "user prefers dark mode", "category": "prefs" }),
        )
        .await
        .unwrap();
        assert_eq!(out["decomposed"], false);
        assert_eq!(out["children"].as_array().unwrap().len(), 0);
        let cap = out["capture_id"].as_str().unwrap();
        let lineage = db.list_by_capture(cap).unwrap();
        assert_eq!(lineage.len(), 1);
        assert_eq!(lineage[0].content, "user prefers dark mode");
        assert_eq!(lineage[0].source, "capture");
    }

    #[tokio::test]
    async fn auto_capture_requires_content() {
        let db = MemoryDb::open_in_memory().unwrap();
        let err = auto_capture(db, None, &json!({})).await.unwrap_err();
        assert!(err.contains("missing 'content'"), "got: {err}");
    }

    #[tokio::test]
    async fn auto_capture_decompose_without_context_errors() {
        let db = MemoryDb::open_in_memory().unwrap();
        let err = auto_capture(db, None, &json!({ "content": "x", "decompose": true }))
            .await
            .unwrap_err();
        assert!(err.contains("needs a wired context"), "got: {err}");
    }
}
