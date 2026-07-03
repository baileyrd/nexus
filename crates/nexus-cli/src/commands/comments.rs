//! Comment command handlers — `nexus comments list|create-thread|add-reply|
//! resolve|unresolve|edit-comment|delete-comment|delete-thread`.
//!
//! C74 (#427) — `com.nexus.comments` exposed 7 IPC handlers but the
//! sole frontend consumer was the shell (`commentsApi.ts`); a headless
//! CLI session had to edit note files directly instead of using this
//! non-destructive annotation channel. All eight commands here dispatch
//! through `com.nexus.comments` via `ipc_call`; `create-thread` also
//! reaches `com.nexus.editor` for the open/get_tree/stamp_block anchor
//! chain, the same machinery the shell's comment pane uses.

use anyhow::{Context, Result};
use nexus_types::constants::IPC_TIMEOUT_SHORT as IPC_TIMEOUT;
use nexus_types::plugin_ids;
use serde_json::Value;

use crate::app::App;
use crate::output::OutputFormat;

const COMMENTS_PLUGIN: &str = plugin_ids::COMMENTS;
const EDITOR_PLUGIN: &str = plugin_ids::EDITOR;

/// `nexus comments list <path>` — every thread on a note, with full
/// reply history.
pub fn list(app: &mut App, path: &str) -> Result<()> {
    let format = app.format();
    let response = comments_call(app, "list", serde_json::json!({ "file_path": path }))?;
    match format {
        OutputFormat::Json => print_json(&response),
        OutputFormat::Jsonl => print_jsonl(&response),
        OutputFormat::Text | OutputFormat::Table => print_thread_list(&response),
    }
    Ok(())
}

/// `nexus comments create-thread <path> <body> [--block-index N] [--author NAME]`
/// — start a new thread anchored to a top-level block (0-based,
/// default 0 = the file's first block). A headless caller has no
/// editor selection, so `block-index` is the only anchor a CLI
/// session can offer.
pub fn create_thread(
    app: &mut App,
    path: &str,
    body: &str,
    block_index: Option<u32>,
    author: Option<&str>,
) -> Result<()> {
    let format = app.format();
    let stable_id = resolve_anchor(app, path, block_index.unwrap_or(0) as usize)?;
    let args = serde_json::json!({
        "file_path": path,
        "block_id": stable_id,
        "body": body,
        "author": author,
    });
    let response = comments_call(app, "create_thread", args)?;
    match format {
        OutputFormat::Json => print_json(&response),
        OutputFormat::Jsonl => print_jsonl(&response),
        OutputFormat::Text | OutputFormat::Table => print_thread(&response),
    }
    Ok(())
}

/// `nexus comments add-reply <path> <thread-id> <body> [--author NAME]`
pub fn add_reply(
    app: &mut App,
    path: &str,
    thread_id: &str,
    body: &str,
    author: Option<&str>,
) -> Result<()> {
    let format = app.format();
    let args = serde_json::json!({
        "file_path": path,
        "thread_id": thread_id,
        "body": body,
        "author": author,
    });
    let response = comments_call(app, "add_reply", args)?;
    match format {
        OutputFormat::Json => print_json(&response),
        OutputFormat::Jsonl => print_jsonl(&response),
        OutputFormat::Text | OutputFormat::Table => print_comment(&response),
    }
    Ok(())
}

/// `nexus comments resolve <path> <thread-id> [--author NAME]`
pub fn resolve(app: &mut App, path: &str, thread_id: &str, author: Option<&str>) -> Result<()> {
    set_resolved(app, path, thread_id, true, author)
}

/// `nexus comments unresolve <path> <thread-id> [--author NAME]`
pub fn unresolve(app: &mut App, path: &str, thread_id: &str, author: Option<&str>) -> Result<()> {
    set_resolved(app, path, thread_id, false, author)
}

fn set_resolved(
    app: &mut App,
    path: &str,
    thread_id: &str,
    resolved: bool,
    author: Option<&str>,
) -> Result<()> {
    let format = app.format();
    let args = serde_json::json!({
        "file_path": path,
        "thread_id": thread_id,
        "resolved": resolved,
        "author": author,
    });
    let response = comments_call(app, "set_resolved", args)?;
    match format {
        OutputFormat::Json => print_json(&response),
        OutputFormat::Jsonl => print_jsonl(&response),
        OutputFormat::Text | OutputFormat::Table => print_thread(&response),
    }
    Ok(())
}

/// `nexus comments edit-comment <path> <thread-id> <comment-id> <body>`
pub fn edit_comment(
    app: &mut App,
    path: &str,
    thread_id: &str,
    comment_id: &str,
    body: &str,
) -> Result<()> {
    let format = app.format();
    let args = serde_json::json!({
        "file_path": path,
        "thread_id": thread_id,
        "comment_id": comment_id,
        "body": body,
    });
    let response = comments_call(app, "edit_comment", args)?;
    match format {
        OutputFormat::Json => print_json(&response),
        OutputFormat::Jsonl => print_jsonl(&response),
        OutputFormat::Text | OutputFormat::Table => print_comment(&response),
    }
    Ok(())
}

/// `nexus comments delete-comment <path> <thread-id> <comment-id>` —
/// deleting a thread's only comment leaves an empty thread; use
/// `delete-thread` to remove the whole thread instead.
pub fn delete_comment(
    app: &mut App,
    path: &str,
    thread_id: &str,
    comment_id: &str,
) -> Result<()> {
    let args = serde_json::json!({
        "file_path": path,
        "thread_id": thread_id,
        "comment_id": comment_id,
    });
    comments_call(app, "delete_comment", args)?;
    println!("Deleted comment {comment_id} from thread {thread_id}.");
    Ok(())
}

/// `nexus comments delete-thread <path> <thread-id>`
pub fn delete_thread(app: &mut App, path: &str, thread_id: &str) -> Result<()> {
    let args = serde_json::json!({
        "file_path": path,
        "thread_id": thread_id,
    });
    comments_call(app, "delete_thread", args)?;
    println!("Deleted thread {thread_id}.");
    Ok(())
}

// ── Anchor resolution (create-thread only) ─────────────────────────────────

/// Resolve `block_index` (0-based, into the file's top-level blocks)
/// to a stable block id via `com.nexus.editor`'s open → get_tree →
/// stamp_block chain — mirrors `nexus-mcp`'s `nexus_comment_create_thread`.
fn resolve_anchor(app: &mut App, path: &str, block_index: usize) -> Result<String> {
    editor_call(app, "open", serde_json::json!({ "relpath": path }))
        .with_context(|| "editor open failed")?;

    let tree_value = editor_call(app, "get_tree", serde_json::json!({ "relpath": path }))
        .with_context(|| "editor get_tree failed")?;
    let root_blocks = tree_value
        .get("tree")
        .and_then(|t| t.get("root_blocks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(block_id) = root_blocks.get(block_index).and_then(Value::as_str) else {
        anyhow::bail!(
            "block_index {block_index} out of range — '{path}' has {} top-level block(s)",
            root_blocks.len(),
        );
    };

    let stamp_value = editor_call(
        app,
        "stamp_block",
        serde_json::json!({ "relpath": path, "block_id": block_id }),
    )
    .with_context(|| "editor stamp_block failed")?;
    let stable_id = stamp_value
        .get("stable_id")
        .and_then(Value::as_str)
        .with_context(|| "editor stamp_block: reply missing stable_id")?;
    Ok(stable_id.to_string())
}

// ── Printers ────────────────────────────────────────────────────────────────

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn print_jsonl(v: &Value) {
    println!("{}", serde_json::to_string(v).unwrap_or_default());
}

fn print_thread_list(v: &Value) {
    let threads = v.as_array().cloned().unwrap_or_default();
    if threads.is_empty() {
        println!("No comment threads.");
        return;
    }
    println!("{} thread(s):", threads.len());
    for t in &threads {
        print_thread(t);
        println!();
    }
}

fn print_thread(t: &Value) {
    let id = t.get("id").and_then(Value::as_str).unwrap_or("?");
    let block_id = t.get("block_id").and_then(Value::as_str).unwrap_or("?");
    let resolved = t
        .get("resolved")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    println!(
        "Thread {id}  [{}]  block {block_id}",
        if resolved { "resolved" } else { "open" }
    );
    let comments = t.get("comments").and_then(Value::as_array).cloned().unwrap_or_default();
    for c in &comments {
        print_comment_line(c);
    }
}

fn print_comment(c: &Value) {
    print_comment_line(c);
}

fn print_comment_line(c: &Value) {
    let id = c.get("id").and_then(Value::as_str).unwrap_or("?");
    let author = c.get("author").and_then(Value::as_str).unwrap_or("(anonymous)");
    let body = c.get("body").and_then(Value::as_str).unwrap_or("");
    println!("  [{id}] {author}: {body}");
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn comments_call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (invoker, rt) = app.invoker()?;
    rt.block_on(invoker.ipc_call(COMMENTS_PLUGIN, command, args, IPC_TIMEOUT))
        .with_context(|| format!("comments ipc call '{command}' failed"))
}

fn editor_call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (invoker, rt) = app.invoker()?;
    rt.block_on(invoker.ipc_call(EDITOR_PLUGIN, command, args, IPC_TIMEOUT))
        .with_context(|| format!("editor ipc call '{command}' failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_thread_list_handles_empty_array() {
        // Smoke test — must not panic on an empty threads array.
        print_thread_list(&serde_json::json!([]));
    }

    #[test]
    fn print_thread_handles_missing_fields() {
        // Smoke test — malformed/partial replies must not panic.
        print_thread(&serde_json::json!({}));
    }

    #[test]
    fn print_comment_line_falls_back_to_anonymous() {
        // Smoke test for the no-author path; behavior is verified via
        // stdout capture at the integration level, not here.
        print_comment_line(&serde_json::json!({ "id": "c1", "body": "hi" }));
    }
}
