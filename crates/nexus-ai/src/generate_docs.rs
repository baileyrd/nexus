//! BL-116 — `com.nexus.ai::generate_docs` implementation.
//!
//! Pipeline:
//!
//!   1. Resolve the symbol via `com.nexus.storage::query_symbol`,
//!      addressing by `symbol_id` (preferred) or `path` + `name`.
//!   2. Read the source file with `com.nexus.storage::read_file` and
//!      slice the lines covered by `line_start..=line_end`.
//!   3. Collect 1-hop context — the parent symbol (impl / class / mod)
//!      plus sibling symbols inside the same parent. BL-114's index
//!      has no call-edges yet, so this is a structural proxy for
//!      "what's nearby" rather than a true callers/callees set; the
//!      reply carries a `degraded: true` flag so callers know.
//!   4. Build a structured prompt for the configured AI provider.
//!   5. Dispatch a single `chat` call (no tools, no streaming) and
//!      wrap the result in the language's documentation-comment
//!      syntax (rustdoc / JSDoc / godoc / docstring).
//!
//! Write-back through `com.nexus.editor::apply_transaction` is
//! deliberately NOT performed here — the DoD treats it as "optional"
//! and the splice + undo bookkeeping is the caller's responsibility.
//! The reply echoes `insert_line` (the symbol's `line_start`) so the
//! caller can stitch the docblock in without re-resolving.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::PluginError;

use crate::config::AiConfig;
use crate::ipc::{AiGenerateDocsArgs, AiGenerateDocsReply};
use crate::provider::{ChatMessage, Role};

const STORAGE_PLUGIN: &str = "com.nexus.storage";
const STORAGE_IPC_TIMEOUT: Duration = Duration::from_secs(15);

/// `true` whenever the BL-114 index lacks the data the GitNexus
/// equivalent would consume. v1 always reports `true` so agent
/// prompts can downweight the result's confidence.
pub(crate) const DEGRADED_REASON: &str =
    "BL-114's code-symbol index records declarations only; the 1-hop \
     context here uses parent + sibling symbols as a proxy for direct \
     callers / callees. Call-edge indexing lands in a follow-up BL.";

/// Decoded form of `nexus_storage::ipc::StorageSymbolRow`. Mirrored
/// locally so this module doesn't depend on `nexus-storage` for the
/// type alone — the wire shape is what matters.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct SymbolRow {
    pub id: i64,
    pub path: String,
    pub language: String,
    pub kind: String,
    pub name: String,
    pub line_start: u32,
    pub line_end: u32,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub doc_comment: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct QuerySymbolReply {
    symbols: Vec<SymbolRow>,
}

#[derive(Debug, serde::Deserialize)]
struct ReadFileReply {
    #[serde(default)]
    bytes: Option<Vec<u8>>,
}

/// Comment-syntax flavour selected for the output docblock. Resolved
/// from the symbol's language unless the caller passes an explicit
/// `style` override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocStyle {
    Rustdoc,
    JsDoc,
    Godoc,
    PyDocstring,
}

impl DocStyle {
    pub(crate) fn for_language(lang: &str) -> Self {
        match lang {
            "rust" => Self::Rustdoc,
            "python" => Self::PyDocstring,
            "go" => Self::Godoc,
            _ => Self::JsDoc, // ts / tsx / js / jsx and any future C-family
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "rustdoc" | "rust" => Some(Self::Rustdoc),
            "jsdoc" | "js" | "ts" | "typescript" | "javascript" => Some(Self::JsDoc),
            "godoc" | "go" => Some(Self::Godoc),
            "docstring" | "python" | "py" => Some(Self::PyDocstring),
            _ => None,
        }
    }
}

/// Entry point invoked from `core_plugin::dispatch_async`. Top-level
/// returns a `Result` so existing error-mapping at the dispatch site
/// keeps working.
pub(crate) async fn handle_generate_docs(
    ctx: Arc<KernelPluginContext>,
    ai_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: AiGenerateDocsArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("generate_docs: args decode: {e}")))?;

    let ai_cfg = ai_cfg
        .ok_or_else(|| exec_err("generate_docs: no AI chat provider configured"))?;
    let ai = crate::core_plugin::build_ai_provider(&ai_cfg).map_err(exec_err)?;

    let symbol = resolve_symbol(&ctx, &parsed).await?;
    let source = read_source_snippet(&ctx, &symbol).await?;
    let neighbours = collect_neighbours(&ctx, &symbol).await;

    let style = parsed
        .style
        .as_deref()
        .and_then(DocStyle::parse)
        .unwrap_or_else(|| DocStyle::for_language(&symbol.language));

    let prompt = build_prompt(&symbol, &source, &neighbours, style);
    let messages = vec![ChatMessage {
        role: Role::User,
        content: prompt,
    }];
    let system = Some(SYSTEM_PROMPT);
    let raw = ai
        .chat(&messages, system)
        .await
        .map_err(|e| exec_err(format!("generate_docs: provider: {e}")))?;

    let docblock = format_as_doc_comment(raw.trim(), style);

    let reply = AiGenerateDocsReply {
        docblock,
        symbol_id: symbol.id,
        language: symbol.language,
        kind: symbol.kind,
        name: symbol.name,
        path: symbol.path,
        insert_line: symbol.line_start,
        degraded: true,
        degraded_reason: Some(DEGRADED_REASON.to_string()),
    };
    serde_json::to_value(&reply).map_err(|e| exec_err(format!("generate_docs: encode: {e}")))
}

const SYSTEM_PROMPT: &str = "You are a senior engineer writing documentation for the named \
    symbol. Reply with prose only — no comment markers, no leading whitespace, no greeting. \
    Cover what the symbol does and any non-obvious caveats. Keep it to 1–3 short paragraphs.";

async fn resolve_symbol(
    ctx: &Arc<KernelPluginContext>,
    args: &AiGenerateDocsArgs,
) -> Result<SymbolRow, PluginError> {
    let mut request = serde_json::Map::new();
    request.insert("limit".to_string(), serde_json::json!(50));
    let mut have_filter = false;
    if let Some(name) = args.name.as_ref().filter(|s| !s.is_empty()) {
        request.insert("name".to_string(), serde_json::json!(name));
        have_filter = true;
    }
    if let Some(path) = args.path.as_ref().filter(|s| !s.is_empty()) {
        request.insert("path".to_string(), serde_json::json!(path));
        have_filter = true;
    }
    if !have_filter && args.symbol_id.is_none() {
        return Err(exec_err(
            "generate_docs: provide either `symbol_id`, or `path` + `name`",
        ));
    }
    let raw = ctx
        .ipc_call(
            STORAGE_PLUGIN,
            "query_symbol",
            serde_json::Value::Object(request),
            STORAGE_IPC_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("generate_docs: query_symbol: {e}")))?;
    let reply: QuerySymbolReply = serde_json::from_value(raw)
        .map_err(|e| exec_err(format!("generate_docs: decode query_symbol: {e}")))?;
    // When `symbol_id` is set we still send name/path filters because
    // the storage handler doesn't accept id directly — we filter
    // client-side instead. A 50-row cap is plenty for any realistic
    // name×path combination.
    let chosen = if let Some(want) = args.symbol_id {
        reply.symbols.into_iter().find(|r| r.id == want)
    } else {
        reply.symbols.into_iter().next()
    };
    chosen.ok_or_else(|| {
        exec_err(format!(
            "generate_docs: no symbol matching {args:?} in BL-114 index"
        ))
    })
}

async fn read_source_snippet(
    ctx: &Arc<KernelPluginContext>,
    symbol: &SymbolRow,
) -> Result<String, PluginError> {
    let raw = ctx
        .ipc_call(
            STORAGE_PLUGIN,
            "read_file",
            serde_json::json!({ "path": symbol.path }),
            STORAGE_IPC_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("generate_docs: read_file: {e}")))?;
    let reply: ReadFileReply = serde_json::from_value(raw)
        .map_err(|e| exec_err(format!("generate_docs: decode read_file: {e}")))?;
    let bytes = reply
        .bytes
        .ok_or_else(|| exec_err(format!("generate_docs: source file missing: {}", symbol.path)))?;
    let text = String::from_utf8(bytes)
        .map_err(|e| exec_err(format!("generate_docs: source is not UTF-8: {e}")))?;
    Ok(slice_lines(&text, symbol.line_start, symbol.line_end))
}

/// 1-based inclusive line slice. A symbol whose `line_end` exceeds
/// the file's line count is truncated rather than erroring — keeps a
/// stale index from blocking the docblock generation entirely.
pub(crate) fn slice_lines(text: &str, line_start: u32, line_end: u32) -> String {
    let start = line_start.saturating_sub(1) as usize;
    let end = line_end.saturating_sub(1) as usize;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if start >= lines.len() {
        return String::new();
    }
    let stop = end.min(lines.len().saturating_sub(1));
    lines[start..=stop].concat()
}

/// 1-hop neighbour set: parent (if any) plus sibling symbols sharing
/// the same parent. Failures fall through to an empty set rather
/// than aborting — neighbours are best-effort context, not load-
/// bearing.
pub(crate) async fn collect_neighbours(
    ctx: &Arc<KernelPluginContext>,
    symbol: &SymbolRow,
) -> Vec<SymbolRow> {
    let raw = match ctx
        .ipc_call(
            STORAGE_PLUGIN,
            "query_symbol",
            serde_json::json!({ "path": symbol.path, "limit": 500 }),
            STORAGE_IPC_TIMEOUT,
        )
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(%e, "generate_docs: neighbour query failed");
            return Vec::new();
        }
    };
    let reply: QuerySymbolReply = match serde_json::from_value(raw) {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(%e, "generate_docs: neighbour decode failed");
            return Vec::new();
        }
    };
    let pid = symbol.parent_id;
    reply
        .symbols
        .into_iter()
        .filter(|r| r.id != symbol.id)
        .filter(|r| match pid {
            Some(p) => r.id == p || r.parent_id == Some(p),
            None => false,
        })
        .collect()
}

/// Assemble the user-message prompt. Keeps `system` thin (in
/// `SYSTEM_PROMPT`) and packs all the symbol-specific context into
/// the user turn so the provider's chat session can be fresh each
/// call.
pub(crate) fn build_prompt(
    symbol: &SymbolRow,
    source: &str,
    neighbours: &[SymbolRow],
    style: DocStyle,
) -> String {
    let mut out = String::new();
    out.push_str("Generate documentation for the following ");
    out.push_str(&symbol.language);
    out.push(' ');
    out.push_str(&symbol.kind);
    out.push_str(" named `");
    out.push_str(&symbol.name);
    out.push_str("`.\n\n");
    out.push_str("Doc style: ");
    out.push_str(match style {
        DocStyle::Rustdoc => "rustdoc (`/// …` over the item)",
        DocStyle::JsDoc => "JSDoc (`/** … */` over the item)",
        DocStyle::Godoc => "godoc (`// Name …` paragraph over the item)",
        DocStyle::PyDocstring => "Python docstring (triple-quoted string as the first body statement)",
    });
    out.push_str("\nPath: ");
    out.push_str(&symbol.path);
    let _ = writeln!(out, "\nLines: {}–{}\n", symbol.line_start, symbol.line_end);

    out.push_str("=== Source ===\n");
    out.push_str(source.trim_end());
    out.push('\n');
    out.push_str("=== End source ===\n\n");

    if !neighbours.is_empty() {
        out.push_str("Nearby symbols (parent + siblings — used as proxy for callers/callees \
                     since the index lacks call edges):\n");
        for n in neighbours.iter().take(12) {
            let _ = writeln!(
                out,
                "- {} `{}` ({}:{})",
                n.kind, n.name, n.path, n.line_start
            );
        }
        out.push('\n');
    }

    if let Some(doc) = &symbol.doc_comment {
        if !doc.is_empty() {
            out.push_str("Existing doc comment (improve / rewrite):\n");
            out.push_str(doc);
            out.push_str("\n\n");
        }
    }

    out.push_str(
        "Reply with the documentation prose only. No comment markers \
         like `///` or `/**` — the caller wraps your output in the \
         correct syntax. Do not echo the source code.",
    );
    out
}

/// Wrap raw prose in the symbol's documentation-comment syntax.
/// Returns the docblock as a multi-line string with a trailing
/// newline so the caller can splice it directly above
/// `insert_line`.
pub(crate) fn format_as_doc_comment(prose: &str, style: DocStyle) -> String {
    let trimmed = prose.trim_matches(|c: char| c == '\n' || c == ' ' || c == '\t');
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    match style {
        DocStyle::Rustdoc => {
            for line in trimmed.split('\n') {
                if line.is_empty() {
                    out.push_str("///\n");
                } else {
                    out.push_str("/// ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        DocStyle::Godoc => {
            for line in trimmed.split('\n') {
                if line.is_empty() {
                    out.push_str("//\n");
                } else {
                    out.push_str("// ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        DocStyle::JsDoc => {
            out.push_str("/**\n");
            for line in trimmed.split('\n') {
                if line.is_empty() {
                    out.push_str(" *\n");
                } else {
                    out.push_str(" * ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            out.push_str(" */\n");
        }
        DocStyle::PyDocstring => {
            // Python docstrings sit inside the function/class body
            // as a triple-quoted string. Keep the indentation
            // problem out of scope — the caller knows where the
            // body starts and adjusts.
            out.push_str("\"\"\"\n");
            out.push_str(trimmed);
            if !trimmed.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("\"\"\"\n");
        }
    }
    out
}

fn exec_err(msg: impl Into<String>) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: crate::core_plugin::PLUGIN_ID.to_string(),
        reason: msg.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, name: &str, kind: &str, lang: &str, parent: Option<i64>) -> SymbolRow {
        SymbolRow {
            id,
            path: "src/lib.rs".into(),
            language: lang.into(),
            kind: kind.into(),
            name: name.into(),
            line_start: 10,
            line_end: 14,
            parent_id: parent,
            doc_comment: None,
        }
    }

    #[test]
    fn doc_style_resolves_from_language() {
        assert_eq!(DocStyle::for_language("rust"), DocStyle::Rustdoc);
        assert_eq!(DocStyle::for_language("python"), DocStyle::PyDocstring);
        assert_eq!(DocStyle::for_language("go"), DocStyle::Godoc);
        assert_eq!(DocStyle::for_language("typescript"), DocStyle::JsDoc);
        assert_eq!(DocStyle::for_language("javascript"), DocStyle::JsDoc);
        assert_eq!(DocStyle::for_language("anything-else"), DocStyle::JsDoc);
    }

    #[test]
    fn doc_style_parses_override_strings() {
        assert_eq!(DocStyle::parse("rustdoc"), Some(DocStyle::Rustdoc));
        assert_eq!(DocStyle::parse("RUSTDOC"), Some(DocStyle::Rustdoc));
        assert_eq!(DocStyle::parse("godoc"), Some(DocStyle::Godoc));
        assert_eq!(DocStyle::parse("jsdoc"), Some(DocStyle::JsDoc));
        assert_eq!(DocStyle::parse("docstring"), Some(DocStyle::PyDocstring));
        assert!(DocStyle::parse("bogus").is_none());
    }

    #[test]
    fn slice_lines_picks_inclusive_range() {
        let text = "a\nb\nc\nd\ne\n";
        assert_eq!(slice_lines(text, 2, 4), "b\nc\nd\n");
    }

    #[test]
    fn slice_lines_truncates_past_eof() {
        let text = "a\nb\n";
        // Symbol claims lines 1..=99 — we just give what's there.
        assert_eq!(slice_lines(text, 1, 99), "a\nb\n");
    }

    #[test]
    fn slice_lines_empty_when_start_past_eof() {
        let text = "a\nb\n";
        assert_eq!(slice_lines(text, 10, 12), "");
    }

    #[test]
    fn format_rustdoc_prefixes_each_line() {
        let out = format_as_doc_comment("Summary line.\n\nDetails here.", DocStyle::Rustdoc);
        assert!(out.contains("/// Summary line.\n"));
        assert!(out.contains("///\n"));
        assert!(out.contains("/// Details here.\n"));
    }

    #[test]
    fn format_godoc_uses_double_slash() {
        let out = format_as_doc_comment("Greet says hi.", DocStyle::Godoc);
        assert_eq!(out, "// Greet says hi.\n");
    }

    #[test]
    fn format_jsdoc_wraps_in_block() {
        let out = format_as_doc_comment("First.\nSecond.", DocStyle::JsDoc);
        assert!(out.starts_with("/**\n"));
        assert!(out.contains(" * First.\n"));
        assert!(out.contains(" * Second.\n"));
        assert!(out.ends_with(" */\n"));
    }

    #[test]
    fn format_pydocstring_uses_triple_quotes() {
        let out = format_as_doc_comment("Greet the named user.", DocStyle::PyDocstring);
        assert!(out.starts_with("\"\"\"\n"));
        assert!(out.ends_with("\"\"\"\n"));
        assert!(out.contains("Greet the named user."));
    }

    #[test]
    fn format_empty_prose_returns_empty() {
        assert!(format_as_doc_comment("   \n  ", DocStyle::Rustdoc).is_empty());
    }

    #[test]
    fn build_prompt_includes_symbol_metadata_and_source() {
        let sym = row(1, "Counter", "struct", "rust", None);
        let prompt = build_prompt(&sym, "pub struct Counter;", &[], DocStyle::Rustdoc);
        assert!(prompt.contains("Counter"));
        assert!(prompt.contains("struct"));
        assert!(prompt.contains("rustdoc"));
        assert!(prompt.contains("pub struct Counter"));
        assert!(prompt.contains("Lines: 10–14"));
    }

    #[test]
    fn build_prompt_lists_neighbours_when_present() {
        let sym = row(2, "bump", "method", "rust", Some(10));
        let neighbours = vec![
            row(10, "Counter", "impl", "rust", None),
            row(3, "new", "method", "rust", Some(10)),
        ];
        let prompt = build_prompt(&sym, "fn bump() {}", &neighbours, DocStyle::Rustdoc);
        assert!(prompt.contains("Nearby symbols"));
        assert!(prompt.contains("impl `Counter`"));
        assert!(prompt.contains("method `new`"));
    }

    #[test]
    fn build_prompt_mentions_existing_doc_when_present() {
        let mut sym = row(1, "Counter", "struct", "rust", None);
        sym.doc_comment = Some("Old doc.".to_string());
        let prompt = build_prompt(&sym, "pub struct Counter;", &[], DocStyle::Rustdoc);
        assert!(prompt.contains("Existing doc comment"));
        assert!(prompt.contains("Old doc."));
    }

    #[test]
    fn build_prompt_omits_neighbours_block_when_empty() {
        let sym = row(1, "hello", "function", "rust", None);
        let prompt = build_prompt(&sym, "fn hello() {}", &[], DocStyle::Rustdoc);
        assert!(!prompt.contains("Nearby symbols"));
    }

    #[test]
    fn degraded_reason_documents_the_proxy() {
        assert!(DEGRADED_REASON.contains("BL-114"));
        // Test the prose-shape rather than a hyphenated token so a
        // future rewording ("call edges" → "call graph") doesn't
        // brittle-break this.
        assert!(
            DEGRADED_REASON.contains("call") && DEGRADED_REASON.contains("edge"),
            "DEGRADED_REASON should mention the missing call-edge data",
        );
    }
}
