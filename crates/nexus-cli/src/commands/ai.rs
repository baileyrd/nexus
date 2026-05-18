//! AI command handlers — `nexus ai ask|embed|status|config|chat|complete`.
//!
//! Every AI call routes through `com.nexus.ai` via `ipc_call`; the CLI
//! does not link against `nexus-ai` directly.

use std::io::{self, Write};

use anyhow::{Context, Result};
use nexus_bootstrap::storage as storage_ipc;
use nexus_kernel::{EventFilter, NexusEvent, PluginContext};
use nexus_types::constants::IPC_TIMEOUT_LONG as IPC_TIMEOUT;
use nexus_types::plugin_ids;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde_json::Value;

use crate::app::App;

const AI_PLUGIN: &str = plugin_ids::AI;
/// `com.nexus.ai::stream_chat` (handler id 6) — multi-mode streaming.
const STREAM_CHAT: &str = "stream_chat";
/// Bus prefix every `stream_chat` event uses. Subscribers select by
/// session id encoded in the payload.
const STREAM_PREFIX: &str = "com.nexus.ai.stream_";

/// Exit code for Ctrl-C at the chat prompt — matches the shell's
/// SIGINT-by-default behaviour so a wrapped `nexus ai chat` reads
/// uniformly across `cli` / `tmux` / CI.
const EXIT_INTERRUPTED: i32 = 130;

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
            let (invoker, rt) = app.invoker()?;
            rt.block_on(storage_ipc::query_files(&*invoker))?
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

#[allow(clippy::type_complexity)]
fn fetch_blocks(app: &mut App, path: &str) -> Result<Vec<(u64, String, String, Option<i32>)>> {
    let (invoker, rt) = app.invoker()?;
    let blocks = rt.block_on(storage_ipc::query_blocks(&*invoker, path))?;
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

    let tls_pinned = response
        .get("tls_pinned")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let local_embeddings_supported = response
        .get("local_embeddings_supported")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    println!("AI Provider       : {ai_provider} ({ai_model})");
    println!("Embedding Provider: {embed_provider}");
    println!("Indexed Chunks    : {indexed}");
    println!("TLS Pinned        : {}", if tls_pinned { "yes" } else { "no" });
    println!(
        "Local Embeddings  : {}",
        if local_embeddings_supported {
            "compiled-in"
        } else {
            "not built (rebuild with --features local-embeddings)"
        }
    );

    // FU-10 — surface the BL-041 indexing-daemon snapshot so a
    // headless `nexus ai status` reads as well as the shell badge.
    // Soft-fail: if the handler is unreachable (older bootstrap),
    // skip the line rather than abort the command.
    if let Ok(snap) = call(app, "index_status", serde_json::json!({})) {
        let indexed_files = snap
            .get("indexed_files")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let total_seen = snap.get("total_seen").and_then(Value::as_u64).unwrap_or(0);
        let pending = snap
            .get("pending_files")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        println!("Index Status      : {indexed_files} / {total_seen} (pending: {pending})");
    }

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
    let (invoker, rt) = app.invoker()?;
    rt.block_on(invoker.ipc_call(AI_PLUGIN, command, args, IPC_TIMEOUT))
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

/// Read the current AI config, swap in `model`, push it back. The
/// `set_config` handler is keyed on the full `{ ai, embedding }`
/// shape (provider + model + api_key etc.), so a "set just the model"
/// is implemented as a read-modify-write — same pattern Settings → AI
/// uses on the shell side.
fn override_model(app: &mut App, model: &str) -> Result<()> {
    let response = call(app, "config", serde_json::json!({}))?;
    let mut ai = response
        .get("ai")
        .cloned()
        .filter(Value::is_object)
        .ok_or_else(|| anyhow::anyhow!("no AI provider configured — `nexus ai config` first"))?;
    if let Value::Object(map) = &mut ai {
        map.insert("model".to_string(), Value::String(model.to_owned()));
    }
    let embedding = response.get("embedding").cloned().unwrap_or(Value::Null);
    call(
        app,
        "set_config",
        serde_json::json!({ "ai": ai, "embedding": embedding }),
    )?;
    Ok(())
}

// ── BL-010 — `nexus ai chat` REPL ────────────────────────────────────────────

/// Multi-turn streaming chat against `com.nexus.ai::stream_chat`.
///
/// Layout note: BL-010 keeps every helper that BL-011 also uses (the
/// stream-drain pump and the message/payload builders) in this module
/// without owning them — `complete()` reaches into the same helpers
/// so the wire shape stays byte-for-byte identical between the two
/// surfaces, matching the engine's `EngineEnvelope` contract.
pub fn chat(
    app: &mut App,
    context: Option<&str>,
    model: Option<&str>,
    session: Option<&str>,
    system: Option<&str>,
) -> Result<()> {
    if let Some(name) = model {
        override_model(app, name).context("failed to override model")?;
    }

    let mut history: Vec<Message> = Vec::new();
    let session_id = session
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut system_prompt = system.map(str::to_owned);

    if let Some(path) = context {
        let bytes = {
            let (invoker, rt) = app.invoker()?;
            rt.block_on(storage_ipc::read_file(&*invoker, path))?
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // The seeded turn is a user message so the assistant has the
        // file as context for its first reply — same pattern the
        // shell's chat view uses for "ask about this file".
        history.push(Message::user(format!(
            "File `{path}`:\n```\n{text}\n```"
        )));
    }

    eprintln!("nexus ai chat — session {session_id}");
    eprintln!("Type /help for commands, Ctrl-D or /quit to exit.");

    let mut rl = DefaultEditor::new().context("failed to init line editor")?;
    loop {
        let readline = rl.readline("> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(trimmed);
                if let Some(cmd) = trimmed.strip_prefix('/') {
                    match handle_slash(app, cmd, &mut history, &mut system_prompt)? {
                        SlashOutcome::Continue => continue,
                        SlashOutcome::Quit => {
                            return Ok(());
                        }
                    }
                }
                history.push(Message::user(trimmed.to_owned()));
                let args = build_chat_args(&history, system_prompt.as_deref(), &session_id);
                match stream_and_collect(app, args, &session_id) {
                    Ok(text) => {
                        // Ensure stdout ends on a newline so the next
                        // prompt doesn't end up on the same line.
                        if !text.ends_with('\n') {
                            println!();
                        }
                        history.push(Message::assistant(text));
                    }
                    Err(err) => {
                        // API errors keep the session alive — the user
                        // can /clear, retry, or quit. Print the error
                        // and remove the unanswered user turn so the
                        // history doesn't drift away from the wire.
                        eprintln!("Error: {err:#}");
                        history.pop();
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C at the prompt — exit 130 so wrapping shells
                // see the canonical "interrupted" code.
                std::process::exit(EXIT_INTERRUPTED);
            }
            Err(ReadlineError::Eof) => return Ok(()),
            Err(err) => return Err(err.into()),
        }
    }
}

#[derive(Clone)]
struct Message {
    role: &'static str,
    content: String,
}

impl Message {
    fn user(content: String) -> Self {
        Self { role: "user", content }
    }
    fn assistant(content: String) -> Self {
        Self { role: "assistant", content }
    }
}

enum SlashOutcome {
    Continue,
    Quit,
}

fn handle_slash(
    app: &mut App,
    cmd: &str,
    history: &mut Vec<Message>,
    system: &mut Option<String>,
) -> Result<SlashOutcome> {
    let mut parts = cmd.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    match head {
        "help" | "?" => {
            println!("/help              show this help");
            println!("/clear             reset history (keeps session id + system prompt)");
            println!("/save <FILE>      append the conversation to <FILE> as Markdown");
            println!("/model <NAME>     update the AI model for subsequent turns");
            println!("/context <FILE>   add the contents of <FILE> as a user turn");
            println!("/system <PROMPT>  replace the system prompt (empty arg clears it)");
            println!("/quit | /exit     leave the chat (exit 0)");
        }
        "clear" => {
            history.clear();
            println!("(history cleared)");
        }
        "quit" | "exit" => return Ok(SlashOutcome::Quit),
        "save" => {
            if rest.is_empty() {
                eprintln!("/save needs a path");
                return Ok(SlashOutcome::Continue);
            }
            let mut buf = String::new();
            for msg in history.iter() {
                buf.push_str(&format!("**{}**:\n\n{}\n\n", msg.role, msg.content));
            }
            std::fs::write(rest, buf).with_context(|| format!("write {rest}"))?;
            println!("(saved to {rest})");
        }
        "model" => {
            if rest.is_empty() {
                eprintln!("/model needs a model name");
                return Ok(SlashOutcome::Continue);
            }
            override_model(app, rest).with_context(|| format!("failed to set ai.model={rest}"))?;
            println!("(model set to {rest})");
        }
        "context" => {
            if rest.is_empty() {
                eprintln!("/context needs a path");
                return Ok(SlashOutcome::Continue);
            }
            let bytes = {
                let (invoker, rt) = app.invoker()?;
                rt.block_on(storage_ipc::read_file(&*invoker, rest))?
            };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            history.push(Message::user(format!("File `{rest}`:\n```\n{text}\n```")));
            println!("(loaded {} bytes from {rest})", bytes.len());
        }
        "system" => {
            *system = if rest.is_empty() { None } else { Some(rest.to_owned()) };
            println!("(system prompt {})", if rest.is_empty() { "cleared" } else { "updated" });
        }
        other => {
            eprintln!("unknown command: /{other} — try /help");
        }
    }
    Ok(SlashOutcome::Continue)
}

fn build_chat_args(history: &[Message], system: Option<&str>, session_id: &str) -> Value {
    let messages: Vec<Value> = history
        .iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();
    let mut args = serde_json::json!({
        "messages": messages,
        "session_id": session_id,
        "mode": "chat",
    });
    if let Some(sys) = system {
        args["system"] = Value::String(sys.to_owned());
    }
    args
}

// ── BL-011 — `nexus ai complete` ─────────────────────────────────────────────

/// Headless completion at a position inside a forge-relative file.
/// Mirrors BL-034's ghost-completion engine call so the CLI and shell
/// produce identical suggestions for the same prompt.
pub fn complete(
    app: &mut App,
    file: &str,
    line: Option<usize>,
    col: Option<usize>,
    context_lines: Option<usize>,
) -> Result<()> {
    let bytes = {
        let (invoker, rt) = app.invoker()?;
        rt.block_on(storage_ipc::read_file(&*invoker, file))?
    };
    let text = String::from_utf8_lossy(&bytes);

    let prompt = slice_prompt(&text, line, col, context_lines);
    if prompt.is_empty() {
        return Err(anyhow::anyhow!(
            "complete: empty prompt — file is empty or position is at byte 0"
        ));
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let args = serde_json::json!({
        "messages": [{ "role": "user", "content": prompt }],
        "session_id": session_id,
        "mode": "complete",
        "tools": "none",
        "trim": true,
        "max_tokens": 64,
        "stop": ["\n\n"],
    });
    let text = stream_and_collect(app, args, &session_id)?;
    // Trailing newline so the suggestion lines up nicely when the CLI
    // is piped into another tool — the engine's post-process trims
    // dangling whitespace, which would otherwise drop the LF too.
    println!("{text}");
    Ok(())
}

/// Slice the prompt to send to `mode=complete`. With no `line`/`col`
/// the whole file is used; otherwise the prompt is the prefix up to
/// `(line, col)`. `context_lines` clamps the kept prefix to the last
/// N lines so a 10k-line file doesn't blow the prompt budget.
fn slice_prompt(
    text: &str,
    line: Option<usize>,
    col: Option<usize>,
    context_lines: Option<usize>,
) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let target_line = line.map(|l| l.saturating_sub(1).min(lines.len().saturating_sub(1)));

    let mut buf = String::new();
    if let Some(idx) = target_line {
        for raw in lines.iter().take(idx) {
            buf.push_str(raw);
        }
        // Final partial line up to col (if given). col is 1-based,
        // matching how editors expose cursor positions.
        if let Some(line_text) = lines.get(idx) {
            let stripped = line_text.strip_suffix('\n').unwrap_or(line_text);
            match col {
                Some(c) => {
                    let take = c.saturating_sub(1).min(stripped.len());
                    // Walk char boundaries so we never split a multi-byte
                    // codepoint mid-codepoint.
                    let take_bytes = stripped
                        .char_indices()
                        .nth(take)
                        .map(|(b, _)| b)
                        .unwrap_or(stripped.len());
                    buf.push_str(&stripped[..take_bytes]);
                }
                None => buf.push_str(line_text),
            }
        }
    } else {
        buf.push_str(text);
    }

    // Trim leading whole lines if the caller capped the context window.
    if let Some(cap) = context_lines {
        let kept_lines: Vec<&str> = buf.split_inclusive('\n').collect();
        if kept_lines.len() > cap {
            let drop_n = kept_lines.len() - cap;
            buf = kept_lines[drop_n..].concat();
        }
    }
    buf
}

// ── Shared streaming pump (BL-010 + BL-011) ──────────────────────────────────

/// Subscribe to `com.nexus.ai.stream_*`, run the IPC call, and stream
/// matching chunks to stdout. Returns the final text from the IPC
/// response (which equals the post-processed `stream_done` payload).
///
/// Both surfaces go through the same pump so the bus contract is
/// exercised identically by the CLI and the engine's own tests.
fn stream_and_collect(app: &mut App, args: Value, session_id: &str) -> Result<String> {
    let (runtime, rt) = app.runtime()?;
    rt.block_on(async {
        let mut sub = runtime
            .context
            .subscribe(EventFilter::CustomPrefix(STREAM_PREFIX.to_owned()));
        let session = session_id.to_owned();
        let drain = async move {
            let mut out = io::stdout();
            loop {
                match sub.recv().await {
                    Ok(event) => {
                        let NexusEvent::Custom { type_id, payload, .. } = &event.event else {
                            continue;
                        };
                        let evt_session = payload
                            .get("session_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if evt_session != session {
                            continue;
                        }
                        if type_id == "com.nexus.ai.stream_chunk" {
                            if let Some(chunk) = payload.get("chunk").and_then(Value::as_str) {
                                let _ = out.write_all(chunk.as_bytes());
                                let _ = out.flush();
                            }
                        } else if type_id == "com.nexus.ai.stream_done" {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        };
        let drain_handle = tokio::spawn(drain);
        let result = runtime
            .context
            .ipc_call(AI_PLUGIN, STREAM_CHAT, args, IPC_TIMEOUT)
            .await
            .with_context(|| format!("AI ipc call '{STREAM_CHAT}' failed"))?;
        // Wait for the drain task to drink the stream_done event so we
        // never abandon the subscription mid-buffer (would surface as
        // dropped tail tokens on a slow terminal).
        let _ = drain_handle.await;
        let text = result
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default();
        Ok::<String, anyhow::Error>(text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_prompt_no_pos_returns_whole_file() {
        let text = "abc\ndef\n";
        assert_eq!(slice_prompt(text, None, None, None), "abc\ndef\n");
    }

    #[test]
    fn slice_prompt_clips_at_line_end_of_line() {
        let text = "alpha\nbeta\ngamma\n";
        // Line 2, no col — keep through end of "beta" (with its
        // trailing newline).
        assert_eq!(slice_prompt(text, Some(2), None, None), "alpha\nbeta\n");
    }

    #[test]
    fn slice_prompt_clips_at_col_inside_line() {
        let text = "hello world\nrest\n";
        // Line 1, col 6 — keep "hello" (5 chars before the col-6 cursor).
        assert_eq!(slice_prompt(text, Some(1), Some(6), None), "hello");
    }

    #[test]
    fn slice_prompt_caps_context_lines_to_last_n() {
        let text = "1\n2\n3\n4\n5\n";
        // Cap to 2 lines — only "4\n" + "5\n" survive, which matches
        // BL-034's "minChars / contextChars" intent for ghost ctx.
        assert_eq!(slice_prompt(text, None, None, Some(2)), "4\n5\n");
    }

    #[test]
    fn slice_prompt_handles_multibyte_col_safely() {
        // Two emoji + one ASCII char — col 3 should land between the
        // second emoji and "z" without splitting a codepoint.
        let text = "🦀🚀z\n";
        let out = slice_prompt(text, Some(1), Some(3), None);
        assert_eq!(out, "🦀🚀");
    }

    #[test]
    fn slice_prompt_overshoot_line_clamps_to_last_line() {
        let text = "one\n";
        // Line 5 in a 1-line file — clamp to the last line, no panic.
        assert_eq!(slice_prompt(text, Some(5), None, None), "one\n");
    }

    #[test]
    fn build_chat_args_round_trips_messages_and_session() {
        let history = vec![
            Message::user("hi".to_owned()),
            Message::assistant("hello".to_owned()),
        ];
        let args = build_chat_args(&history, Some("be terse"), "session-1");
        assert_eq!(args["session_id"], "session-1");
        assert_eq!(args["mode"], "chat");
        assert_eq!(args["system"], "be terse");
        assert_eq!(args["messages"][0]["role"], "user");
        assert_eq!(args["messages"][0]["content"], "hi");
        assert_eq!(args["messages"][1]["role"], "assistant");
    }

    #[test]
    fn build_chat_args_omits_system_when_none() {
        let history = vec![Message::user("hi".to_owned())];
        let args = build_chat_args(&history, None, "s");
        assert!(args.get("system").is_none());
    }
}
