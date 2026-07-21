//! Chat-session persistence handlers — `session_load`, `session_save`,
//! `session_list`, `session_delete`. Sessions are JSON files under
//! `<forge>/.forge/chat/sessions/<id>.json` (multi-session) or
//! `<forge>/.forge/chat_session.json` (legacy single-session).

use nexus_kernel::{FileSystem as _, KernelPluginContext};
use nexus_plugins::PluginError;

use crate::handlers::shared::exec_err;

/// Relative path for the legacy single-session file. Kept for
/// backwards compatibility — callers that omit `id` on
/// `session_load` / `session_save` keep reading/writing this path.
pub(crate) const LEGACY_SESSION_RELPATH: &str = ".forge/chat_session.json";

/// Directory holding multi-session files. Each session lives at
/// `<SESSIONS_DIR>/<id>.json`.
pub(crate) const SESSIONS_DIR: &str = ".forge/chat/sessions";

/// Validate a caller-supplied session id. Keeps the filename safe
/// for disk + prevents path traversal.
pub(crate) fn validate_session_id(id: &str) -> Result<(), PluginError> {
    if id.is_empty() || id.len() > 64 {
        return Err(exec_err("session id must be 1..=64 chars".to_string()));
    }
    let ok = id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(exec_err("session id must match [A-Za-z0-9_-]+".to_string()));
    }
    Ok(())
}

pub(crate) fn session_path(id: Option<&str>) -> Result<std::path::PathBuf, PluginError> {
    match id {
        None => Ok(std::path::PathBuf::from(LEGACY_SESSION_RELPATH)),
        Some(s) => {
            validate_session_id(s)?;
            Ok(std::path::PathBuf::from(SESSIONS_DIR).join(format!("{s}.json")))
        }
    }
}

#[derive(serde::Deserialize, Default)]
pub(crate) struct SessionArgs {
    #[serde(default)]
    pub(crate) id: Option<String>,
}

pub(crate) async fn handle_session_load(
    ctx: &KernelPluginContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: SessionArgs = serde_json::from_value(args.clone()).unwrap_or_default();
    let path = session_path(parsed.id.as_deref())?;
    match ctx.read_file(&path).await {
        Ok(bytes) => {
            let parsed: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|e| exec_err(format!("session_load: invalid JSON on disk: {e}")))?;
            Ok(parsed)
        }
        // No session saved yet — return null rather than erroring so
        // fresh forges don't spam the UI with warnings.
        Err(_) => Ok(serde_json::Value::Null),
    }
}

pub(crate) async fn handle_session_save(
    ctx: &KernelPluginContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    // Caller may pass `{ id, ... }` or a bare session object. Pull
    // `id` out (if present) and persist the whole payload untouched.
    let id = args
        .as_object()
        .and_then(|o| o.get("id"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    if let Some(ref s) = id {
        validate_session_id(s)?;
    }
    let path = session_path(id.as_deref())?;
    let encoded = serde_json::to_vec_pretty(args)
        .map_err(|e| exec_err(format!("session_save: encode: {e}")))?;
    ctx.write_file(&path, &encoded)
        .await
        .map_err(|e| exec_err(format!("session_save: write: {e}")))?;
    Ok(serde_json::json!({ "bytes": encoded.len(), "id": id }))
}

pub(crate) async fn handle_session_list(
    ctx: &KernelPluginContext,
) -> Result<serde_json::Value, PluginError> {
    let dir = std::path::Path::new(SESSIONS_DIR);
    let Ok(entries) = ctx.list_files(dir).await else {
        return Ok(serde_json::Value::Array(Vec::new()));
    };
    let mut out: Vec<serde_json::Value> = Vec::new();
    for path in entries {
        let Some(stem) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| validate_session_id(s).is_ok())
        else {
            continue;
        };
        let Ok(bytes) = ctx.read_file(&path).await else {
            continue;
        };
        let parsed: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let title = parsed
            .get("title")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        let updated_at = parsed
            .get("updated_at")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        out.push(serde_json::json!({
            "id": stem,
            "title": title,
            "updated_at": updated_at,
            "bytes": bytes.len(),
        }));
    }
    Ok(serde_json::Value::Array(out))
}

pub(crate) async fn handle_session_delete(
    ctx: &KernelPluginContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    #[derive(serde::Deserialize)]
    struct Args {
        id: String,
    }
    let a: Args = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("session_delete: invalid args: {e}")))?;
    validate_session_id(&a.id)?;
    let path = session_path(Some(&a.id))?;
    match ctx.delete_file(&path).await {
        Ok(()) => Ok(serde_json::json!({ "deleted": true, "id": a.id })),
        Err(e) => Err(exec_err(format!("session_delete: {e}"))),
    }
}

/// #384 — render a persisted session as markdown so it can be saved as
/// a first-class forge note. Mirrors `nexus_agent::memory::export_markdown`
/// (`memory_export`, handler 23): pure read + render, no frontmatter, no
/// disk write of its own — the caller decides where the markdown lands.
pub(crate) async fn handle_session_export(
    ctx: &KernelPluginContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: SessionArgs = serde_json::from_value(args.clone()).unwrap_or_default();
    let path = session_path(parsed.id.as_deref())?;
    let bytes = ctx
        .read_file(&path)
        .await
        .map_err(|e| exec_err(format!("session_export: no session found: {e}")))?;
    let session: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| exec_err(format!("session_export: invalid JSON on disk: {e}")))?;
    let markdown = render_session_markdown(&session);
    Ok(serde_json::json!({ "markdown": markdown }))
}

/// Render a persisted chat session — opaque JSON shaped by the shell
/// (see [`handle_session_save`]'s doc comment) — as markdown. Every
/// `user` turn's `question` and every `assistant` turn's `finalText`
/// become a labeled paragraph, in order; an assistant turn with no
/// `finalText` (cancelled mid-stream, or errored) renders a placeholder
/// rather than being silently dropped.
fn render_session_markdown(session: &serde_json::Value) -> String {
    use std::fmt::Write as _;

    let title = session
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty());
    let turns: Vec<serde_json::Value> = session
        .get("turns")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = String::new();
    match title {
        Some(t) => writeln!(out, "# {t}").expect("writeln! to String is infallible"),
        None => writeln!(out, "# Chat session").expect("writeln! to String is infallible"),
    }
    writeln!(out).expect("writeln! to String is infallible");
    writeln!(out, "{} turns.", turns.len()).expect("writeln! to String is infallible");
    writeln!(out).expect("writeln! to String is infallible");

    for turn in &turns {
        let kind = turn.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        match kind {
            "user" => {
                let question = turn.get("question").and_then(|v| v.as_str()).unwrap_or("");
                writeln!(out, "**User:** {question}").expect("writeln! to String is infallible");
            }
            "assistant" => {
                let text = turn
                    .get("finalText")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty());
                match text {
                    Some(t) => writeln!(out, "**Assistant:** {t}")
                        .expect("writeln! to String is infallible"),
                    None => writeln!(out, "**Assistant:** _(no response)_")
                        .expect("writeln! to String is infallible"),
                }
            }
            other => {
                writeln!(out, "_(unrecognized turn kind `{other}`)_")
                    .expect("writeln! to String is infallible");
            }
        }
        writeln!(out).expect("writeln! to String is infallible");
    }

    out
}

#[cfg(test)]
mod render_tests {
    use super::render_session_markdown;
    use serde_json::json;

    #[test]
    fn uses_title_as_the_heading_when_present() {
        let md = render_session_markdown(&json!({ "title": "My Chat", "turns": [] }));
        assert!(md.starts_with("# My Chat\n"));
    }

    #[test]
    fn falls_back_to_a_generic_heading_when_title_is_missing_or_blank() {
        let no_title = render_session_markdown(&json!({ "turns": [] }));
        assert!(no_title.starts_with("# Chat session\n"));
        let blank_title = render_session_markdown(&json!({ "title": "   ", "turns": [] }));
        assert!(blank_title.starts_with("# Chat session\n"));
    }

    #[test]
    fn renders_the_turn_count() {
        let md = render_session_markdown(&json!({
            "turns": [
                { "kind": "user", "question": "hi" },
                { "kind": "assistant", "finalText": "hello" },
            ],
        }));
        assert!(md.contains("2 turns."));
    }

    #[test]
    fn renders_user_and_assistant_turns_in_order() {
        let md = render_session_markdown(&json!({
            "turns": [
                { "kind": "user", "question": "What's the weather?" },
                { "kind": "assistant", "finalText": "Sunny." },
            ],
        }));
        let user_pos = md.find("**User:** What's the weather?").expect("user line");
        let asst_pos = md.find("**Assistant:** Sunny.").expect("assistant line");
        assert!(user_pos < asst_pos, "user turn should render before assistant turn");
    }

    #[test]
    fn assistant_turn_with_no_final_text_gets_a_placeholder() {
        let md = render_session_markdown(&json!({
            "turns": [
                { "kind": "assistant", "finalText": null },
            ],
        }));
        assert!(md.contains("**Assistant:** _(no response)_"));
    }

    #[test]
    fn unknown_turn_kind_gets_a_placeholder_instead_of_being_dropped() {
        let md = render_session_markdown(&json!({
            "turns": [ { "kind": "system-note" } ],
        }));
        assert!(md.contains("_(unrecognized turn kind `system-note`)_"));
    }

    #[test]
    fn empty_session_still_renders_a_heading_and_zero_count() {
        let md = render_session_markdown(&json!({}));
        assert!(md.contains("# Chat session"));
        assert!(md.contains("0 turns."));
    }
}
