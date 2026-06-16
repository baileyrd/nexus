//! Import chat / discussion logs (e.g. Claude exports) into the native store.
//!
//! Accepts the shapes `remind_me`'s importer handles:
//!   - a Claude conversation object — `{ "chat_messages": [ { sender|role, content|text } ] }`
//!   - a list of conversations (each with `chat_messages` or `messages`)
//!   - a single message object — `{ role|sender, content|text }`
//!   - JSONL — one JSON value (message or conversation) per line
//!
//! Each message becomes an episodic [`Memory`] (`source = "import"`), with the
//! role recorded in tags + metadata. A Claude `content` array of
//! `{ "type": "text", "text": … }` blocks is flattened to text.

use std::path::Path;

use serde_json::Value;

use super::ImportReport;
use crate::db::{MemoryDb, Result};
use crate::model::{Memory, MemoryType};

/// Import every message from a chat-log file at `source` into `target`.
///
/// # Errors
/// Returns an error if the file cannot be read or a memory cannot be written.
pub fn import_chat_log(target: &MemoryDb, source: &Path) -> Result<ImportReport> {
    let text = std::fs::read_to_string(source)?;
    let mut report = ImportReport::default();
    for (role, content) in parse(&text) {
        if content.trim().is_empty() {
            report.skipped += 1;
            continue;
        }
        let mut m = Memory::new(content)
            .with_source("import")
            .with_type(MemoryType::Episodic)
            .with_client("claude")
            .with_category("chat")
            .with_tags([role.clone()]);
        m.metadata = serde_json::json!({ "role": role });
        target.insert(&m)?;
        report.imported += 1;
    }
    Ok(report)
}

/// Extract `(role, content)` pairs from a chat-log body. Tries whole-file JSON
/// first, then falls back to JSONL (one JSON value per line).
fn parse(text: &str) -> Vec<(String, String)> {
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        let mut out = Vec::new();
        collect(&v, &mut out);
        if !out.is_empty() {
            return out;
        }
    }
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            collect(&v, &mut out);
        }
    }
    out
}

/// Recursively collect messages from a value: an array (of conversations or
/// messages), a conversation object (`chat_messages`/`messages`), or a single
/// message object (`role`/`sender`).
fn collect(v: &Value, out: &mut Vec<(String, String)>) {
    match v {
        Value::Array(items) => {
            for item in items {
                collect(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(Value::Array(msgs)) =
                map.get("chat_messages").or_else(|| map.get("messages"))
            {
                for msg in msgs {
                    push_message(msg, out);
                }
            } else if map.contains_key("role") || map.contains_key("sender") {
                push_message(v, out);
            }
        }
        _ => {}
    }
}

fn push_message(msg: &Value, out: &mut Vec<(String, String)>) {
    let Value::Object(m) = msg else {
        return;
    };
    let role = m
        .get("sender")
        .or_else(|| m.get("role"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    // Push even empty-content messages — the import loop counts them as
    // `skipped` so the report reflects the full source message count.
    let content = extract_content(m.get("content").or_else(|| m.get("text")));
    out.push((role, content));
}

/// Flatten a `content` field: a plain string, or an array of `{ type, text }`
/// blocks (Claude format) joined by newlines.
fn extract_content(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| {
                b.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| b.as_str())
                    .map(str::to_string)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_claude_conversation_object() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("conv.json");
        std::fs::write(
            &p,
            r#"{"chat_messages":[
                {"sender":"human","content":"How do I deploy to Kubernetes?"},
                {"sender":"assistant","content":[{"type":"text","text":"Use kubectl apply."}]}
            ]}"#,
        )
        .unwrap();
        let db = MemoryDb::open_in_memory().unwrap();
        let report = import_chat_log(&db, &p).unwrap();
        assert_eq!(report.imported, 2);
        let hits = db.search("kubectl", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].memory_type, MemoryType::Episodic);
        assert_eq!(hits[0].source, "import");
        assert_eq!(hits[0].metadata["role"], "assistant");
    }

    #[test]
    fn imports_jsonl_messages() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("log.jsonl");
        std::fs::write(
            &p,
            "{\"role\":\"user\",\"content\":\"first note about otters\"}\n\
             {\"role\":\"assistant\",\"text\":\"second note about badgers\"}\n",
        )
        .unwrap();
        let db = MemoryDb::open_in_memory().unwrap();
        let report = import_chat_log(&db, &p).unwrap();
        assert_eq!(report.imported, 2);
        assert_eq!(db.search("otters", 10).unwrap().len(), 1);
        assert_eq!(db.search("badgers", 10).unwrap().len(), 1);
    }

    #[test]
    fn handles_conversation_array_and_skips_empty() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("convs.json");
        std::fs::write(
            &p,
            r#"[{"chat_messages":[{"role":"user","content":"hello world"},{"role":"assistant","content":"   "}]}]"#,
        )
        .unwrap();
        let db = MemoryDb::open_in_memory().unwrap();
        let report = import_chat_log(&db, &p).unwrap();
        assert_eq!(report.imported, 1);
        assert_eq!(report.skipped, 1);
    }
}
