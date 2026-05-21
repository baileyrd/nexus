//! BL-114 — Code-symbol index. Tree-sitter walker over the BL-075
//! default code-extension set (Rust / TS+TSX / JS+JSX / Python / Go).
//!
//! Symbols extracted by this module land in the `code_symbols` SQLite
//! table (migration 8). One row per declared function / class / struct
//! / interface / impl / etc., with `parent_id` chaining methods back
//! to their enclosing type. The walker is invoked from the storage
//! engine's `write_file` (live updates) and `rebuild_index` (full
//! rebuild) paths, plus from the `com.nexus.git.commit` subscription
//! in [`crate::core_plugin::StorageCorePlugin`] so a pull / rebase /
//! external checkout doesn't leave the index stale.
//!
//! Files on disk remain the source of truth — the table is fully
//! rebuildable from the forge's source files, mirroring the FTS
//! invariant.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use crate::StorageError;

/// Languages this module knows how to extract symbols for.
///
/// Mirrors the BL-075 `DEFAULT_CODE_EXTENSIONS` shell list (minus the
/// structured-config extensions JSON / YAML / TOML, which have no
/// useful symbol surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeLanguage {
    /// Rust source (`.rs`).
    Rust,
    /// TypeScript source (`.ts`).
    TypeScript,
    /// TypeScript+JSX (`.tsx`).
    Tsx,
    /// JavaScript source (`.js` / `.mjs` / `.cjs`).
    JavaScript,
    /// JavaScript+JSX (`.jsx`).
    Jsx,
    /// Python source (`.py`).
    Python,
    /// Go source (`.go`).
    Go,
}

impl CodeLanguage {
    /// Stable string label for the SQLite `language` column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::JavaScript => "javascript",
            Self::Jsx => "jsx",
            Self::Python => "python",
            Self::Go => "go",
        }
    }
}

/// Default code-file extensions mirrored from
/// `shell/src/plugins/nexus/editor/codeMode.ts`. The shell-side
/// override (`nexus.editor.codeFileExtensions`) widens this set at
/// runtime; the Rust default ships the same out-of-the-box list so a
/// fresh forge builds an index without further configuration.
pub const DEFAULT_CODE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go",
];

/// Detect the language for a forge-relative path. Returns `None` for
/// non-code files (markdown, JSON, attachments, etc.).
#[must_use]
pub fn detect_language(path: &str) -> Option<CodeLanguage> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "rs" => CodeLanguage::Rust,
        "ts" => CodeLanguage::TypeScript,
        "tsx" => CodeLanguage::Tsx,
        "js" | "mjs" | "cjs" => CodeLanguage::JavaScript,
        "jsx" => CodeLanguage::Jsx,
        "py" => CodeLanguage::Python,
        "go" => CodeLanguage::Go,
        _ => return None,
    })
}

/// One extracted symbol prior to insertion. Mirrors the SQLite row
/// but uses Vec indices for parent links — the insert phase resolves
/// indices to row ids.
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    /// Symbol kind (`"function"`, `"struct"`, `"impl"`, …).
    pub kind: String,
    /// Identifier as it appears in source.
    pub name: String,
    /// 1-based starting line number (inclusive).
    pub line_start: u32,
    /// 1-based ending line number (inclusive).
    pub line_end: u32,
    /// Index into the `Vec<ExtractedSymbol>` of the enclosing symbol,
    /// or `None` for top-level items.
    pub parent_idx: Option<usize>,
    /// Leading doc comment (rustdoc / godoc / JSDoc / docstring),
    /// trimmed and joined with `\n`. `None` when no doc was detected.
    pub doc_comment: Option<String>,
}

/// One row returned by [`query_symbols`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRecord {
    /// Row id in the `code_symbols` table.
    pub id: i64,
    /// Forge-relative path of the source file.
    pub path: String,
    /// Language label (matches [`CodeLanguage::as_str`]).
    pub language: String,
    /// Symbol kind.
    pub kind: String,
    /// Identifier.
    pub name: String,
    /// 1-based starting line.
    pub line_start: u32,
    /// 1-based ending line.
    pub line_end: u32,
    /// Row id of the enclosing symbol, or `None` for top-level.
    pub parent_id: Option<i64>,
    /// Leading doc comment, if any.
    pub doc_comment: Option<String>,
}

/// Filter for [`query_symbols`]. `name` and `path` AND-combine; an
/// empty filter returns every indexed symbol up to `limit`.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct SymbolFilter {
    /// Exact identifier match. Case-sensitive.
    #[serde(default)]
    pub name: Option<String>,
    /// Exact forge-relative path match (scopes by containing file).
    #[serde(default)]
    pub path: Option<String>,
    /// Maximum rows to return. Defaults to 200.
    #[serde(default)]
    pub limit: Option<u32>,
}

const DEFAULT_QUERY_LIMIT: u32 = 200;

// ── Public API ────────────────────────────────────────────────────────────────

/// Extract every supported symbol from `source` as the given
/// `language`. Returns the symbols in walk order; methods follow the
/// enclosing type / impl / class via `parent_idx`.
#[must_use]
pub fn extract_symbols(language: CodeLanguage, source: &str) -> Vec<ExtractedSymbol> {
    let mut parser = Parser::new();
    let ts_language: tree_sitter::Language = match language {
        CodeLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        CodeLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        CodeLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        CodeLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        CodeLanguage::Jsx => tree_sitter_javascript::LANGUAGE.into(),
        CodeLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        CodeLanguage::Go => tree_sitter_go::LANGUAGE.into(),
    };
    if parser.set_language(&ts_language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let root = tree.root_node();
    match language {
        CodeLanguage::Rust => walk_rust(root, bytes, &mut out, None),
        CodeLanguage::TypeScript
        | CodeLanguage::Tsx
        | CodeLanguage::JavaScript
        | CodeLanguage::Jsx => walk_js_ts(root, bytes, &mut out, None),
        CodeLanguage::Python => walk_python(root, bytes, &mut out, None),
        CodeLanguage::Go => walk_go(root, bytes, &mut out, None),
    }
    out
}

/// Replace every row in `code_symbols` for `path` with `symbols`.
/// Atomic: either every row lands or none do. `language` is stored
/// verbatim per row so a future cross-language query can filter on
/// it without re-parsing.
///
/// # Errors
/// Returns [`StorageError::Database`] on any SQLite failure.
pub fn upsert_file_symbols(
    conn: &Connection,
    path: &str,
    language: CodeLanguage,
    symbols: &[ExtractedSymbol],
) -> Result<usize, StorageError> {
    let tx = conn.unchecked_transaction()?;
    let n = upsert_file_symbols_in_tx(&tx, path, language, symbols)?;
    tx.commit()?;
    Ok(n)
}

/// Same as [`upsert_file_symbols`] but assumes the caller already owns
/// an enclosing transaction (or savepoint) on `conn`. Use this when the
/// upsert is one step inside a larger atomic write — the outer commit
/// covers atomicity for the whole sequence and a nested
/// `BEGIN DEFERRED` would fail at the SQLite layer.
///
/// # Errors
/// Returns [`StorageError::Database`] on any SQLite failure.
pub fn upsert_file_symbols_in_tx(
    conn: &Connection,
    path: &str,
    language: CodeLanguage,
    symbols: &[ExtractedSymbol],
) -> Result<usize, StorageError> {
    conn.execute(
        "DELETE FROM code_symbols WHERE path = ?1",
        rusqlite::params![path],
    )?;
    let mut row_ids: Vec<i64> = Vec::with_capacity(symbols.len());
    {
        let mut stmt = conn.prepare(
            "INSERT INTO code_symbols
                 (path, language, kind, name, line_start, line_end,
                  parent_id, doc_comment, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())",
        )?;
        for sym in symbols {
            let parent_row_id = sym
                .parent_idx
                .and_then(|idx| row_ids.get(idx).copied());
            stmt.execute(rusqlite::params![
                path,
                language.as_str(),
                sym.kind,
                sym.name,
                i64::from(sym.line_start),
                i64::from(sym.line_end),
                parent_row_id,
                sym.doc_comment,
            ])?;
            row_ids.push(conn.last_insert_rowid());
        }
    }
    Ok(symbols.len())
}

/// Remove every row for `path` from `code_symbols`. No-op when the
/// path has no rows.
///
/// # Errors
/// Returns [`StorageError::Database`] on any SQLite failure.
pub fn delete_file_symbols(conn: &Connection, path: &str) -> Result<usize, StorageError> {
    let n = conn.execute(
        "DELETE FROM code_symbols WHERE path = ?1",
        rusqlite::params![path],
    )?;
    Ok(n)
}

/// Query symbols by name and / or containing path. Combines the two
/// filters with AND; passing both empty returns every row up to
/// `limit`. Results are ordered by `(path, line_start)` for stable
/// pagination.
///
/// # Errors
/// Returns [`StorageError::Database`] on any SQLite failure.
pub fn query_symbols(
    conn: &Connection,
    filter: &SymbolFilter,
) -> Result<Vec<SymbolRecord>, StorageError> {
    let mut sql = String::from(
        "SELECT id, path, language, kind, name, line_start, line_end,
                parent_id, doc_comment
           FROM code_symbols
          WHERE 1 = 1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(name) = filter.name.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND name = ?");
        params.push(Box::new(name.clone()));
    }
    if let Some(path) = filter.path.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND path = ?");
        params.push(Box::new(path.clone()));
    }
    sql.push_str(" ORDER BY path ASC, line_start ASC LIMIT ?");
    let limit = filter.limit.unwrap_or(DEFAULT_QUERY_LIMIT);
    params.push(Box::new(i64::from(limit)));

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(AsRef::as_ref).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(SymbolRecord {
            id: row.get(0)?,
            path: row.get(1)?,
            language: row.get(2)?,
            kind: row.get(3)?,
            name: row.get(4)?,
            line_start: row.get::<_, i64>(5)?.try_into().unwrap_or(0),
            line_end: row.get::<_, i64>(6)?.try_into().unwrap_or(0),
            parent_id: row.get(7)?,
            doc_comment: row.get(8)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Total number of rows currently in the `code_symbols` table. Used
/// by tests and the rebuild summary.
///
/// # Errors
/// Returns [`StorageError::Database`] on any SQLite failure.
pub fn count_symbols(conn: &Connection) -> Result<usize, StorageError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM code_symbols",
        [],
        |r| r.get(0),
    )?;
    Ok(usize::try_from(n).unwrap_or(0))
}

// ── Per-language walkers ──────────────────────────────────────────────────────

fn child_text(node: Node<'_>, field: &str, src: &[u8]) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    child.utf8_text(src).ok().map(str::to_string)
}

fn line_start(node: Node<'_>) -> u32 {
    u32::try_from(node.start_position().row).unwrap_or(u32::MAX).saturating_add(1)
}

fn line_end(node: Node<'_>) -> u32 {
    u32::try_from(node.end_position().row).unwrap_or(u32::MAX).saturating_add(1)
}

/// Collect a run of leading line / block comments preceding `node`.
/// `comment_kinds` lists the tree-sitter node kinds counted as comment
/// trivia for this language. `is_doc` is a predicate over the literal
/// comment text: rustdoc keeps only `///` lines, godoc keeps `//`,
/// JSDoc keeps only `/** */` blocks. When `is_doc` returns false the
/// comment is treated as separator, halting the walk so a stray `//
/// TODO` doesn't accidentally become the doc.
fn leading_doc<F>(
    node: Node<'_>,
    src: &[u8],
    comment_kinds: &[&str],
    is_doc: F,
) -> Option<String>
where
    F: Fn(&str) -> bool,
{
    let mut prev = node.prev_sibling()?;
    let mut buf: Vec<String> = Vec::new();
    loop {
        let kind = prev.kind();
        if !comment_kinds.contains(&kind) {
            break;
        }
        let text = prev.utf8_text(src).unwrap_or("");
        if !is_doc(text) {
            break;
        }
        buf.push(text.to_string());
        match prev.prev_sibling() {
            Some(p) => prev = p,
            None => break,
        }
    }
    if buf.is_empty() {
        return None;
    }
    buf.reverse();
    Some(buf.join("\n"))
}

// ─── Rust ──────────────────────────────────────────────────────────────────────

fn walk_rust(node: Node<'_>, src: &[u8], out: &mut Vec<ExtractedSymbol>, parent: Option<usize>) {
    fn push_named(
        kind: &str,
        node: Node<'_>,
        src: &[u8],
        out: &mut Vec<ExtractedSymbol>,
        parent: Option<usize>,
    ) -> Option<usize> {
        let name = child_text(node, "name", src)?;
        let doc = leading_doc(node, src, &["line_comment", "block_comment"], |text| {
            text.starts_with("///") || text.starts_with("//!") || text.starts_with("/**")
        });
        out.push(ExtractedSymbol {
            kind: kind.to_string(),
            name,
            line_start: line_start(node),
            line_end: line_end(node),
            parent_idx: parent,
            doc_comment: doc,
        });
        Some(out.len() - 1)
    }
    match node.kind() {
        "function_item" | "function_signature_item" => {
            push_named("function", node, src, out, parent);
            return;
        }
        "struct_item" => {
            push_named("struct", node, src, out, parent);
            return;
        }
        "enum_item" => {
            push_named("enum", node, src, out, parent);
            return;
        }
        "trait_item" => {
            let idx = push_named("trait", node, src, out, parent);
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.named_children(&mut cursor) {
                    walk_rust(child, src, out, idx);
                }
            }
            return;
        }
        "impl_item" => {
            // Name an impl block by its target type. Trait impls use
            // `<trait> for <type>`; bare impls use just `<type>`.
            let name = child_text(node, "type", src)
                .or_else(|| child_text(node, "trait", src))
                .unwrap_or_else(|| "impl".to_string());
            out.push(ExtractedSymbol {
                kind: "impl".to_string(),
                name,
                line_start: line_start(node),
                line_end: line_end(node),
                parent_idx: parent,
                doc_comment: None,
            });
            let idx = Some(out.len() - 1);
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.named_children(&mut cursor) {
                    walk_rust(child, src, out, idx);
                }
            }
            return;
        }
        "mod_item" => {
            let idx = push_named("module", node, src, out, parent);
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.named_children(&mut cursor) {
                    walk_rust(child, src, out, idx);
                }
            }
            return;
        }
        "const_item" => {
            push_named("const", node, src, out, parent);
            return;
        }
        "static_item" => {
            push_named("static", node, src, out, parent);
            return;
        }
        "type_item" => {
            push_named("type_alias", node, src, out, parent);
            return;
        }
        "union_item" => {
            push_named("union", node, src, out, parent);
            return;
        }
        "macro_definition" => {
            push_named("macro", node, src, out, parent);
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_rust(child, src, out, parent);
    }
}

// ─── JS / TS ───────────────────────────────────────────────────────────────────

fn walk_js_ts(node: Node<'_>, src: &[u8], out: &mut Vec<ExtractedSymbol>, parent: Option<usize>) {
    fn js_doc(node: Node<'_>, src: &[u8]) -> Option<String> {
        // JSDoc sits before the `export` wrapper, not the inner
        // declaration, so look up one level if the parent is an
        // `export_statement` (TS) or `export_default_declaration` (JS
        // default exports).
        let mut anchor = node;
        if let Some(parent) = node.parent() {
            if matches!(
                parent.kind(),
                "export_statement" | "export_default_declaration"
            ) {
                anchor = parent;
            }
        }
        leading_doc(anchor, src, &["comment"], |text| text.starts_with("/**"))
    }
    fn push(
        kind: &str,
        name: String,
        node: Node<'_>,
        src: &[u8],
        out: &mut Vec<ExtractedSymbol>,
        parent: Option<usize>,
    ) -> Option<usize> {
        out.push(ExtractedSymbol {
            kind: kind.to_string(),
            name,
            line_start: line_start(node),
            line_end: line_end(node),
            parent_idx: parent,
            doc_comment: js_doc(node, src),
        });
        Some(out.len() - 1)
    }
    match node.kind() {
        "function_declaration" | "generator_function_declaration" => {
            if let Some(name) = child_text(node, "name", src) {
                push("function", name, node, src, out, parent);
            }
            return;
        }
        "class_declaration" => {
            let idx = if let Some(name) = child_text(node, "name", src) {
                push("class", name, node, src, out, parent)
            } else {
                None
            };
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.named_children(&mut cursor) {
                    walk_js_ts(child, src, out, idx);
                }
            }
            return;
        }
        "method_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = name_node.utf8_text(src).unwrap_or("").to_string();
                if !name.is_empty() {
                    push("method", name, node, src, out, parent);
                }
            }
            return;
        }
        "interface_declaration" => {
            if let Some(name) = child_text(node, "name", src) {
                push("interface", name, node, src, out, parent);
            }
            return;
        }
        "type_alias_declaration" => {
            if let Some(name) = child_text(node, "name", src) {
                push("type_alias", name, node, src, out, parent);
            }
            return;
        }
        "enum_declaration" => {
            if let Some(name) = child_text(node, "name", src) {
                push("enum", name, node, src, out, parent);
            }
            return;
        }
        // `export const Foo = ...` and `const Foo = () => ...` — only
        // surface lexical declarations whose initializer is a
        // function-like expression. Plain data constants are noise.
        "lexical_declaration" | "variable_declaration" => {
            let mut cursor = node.walk();
            for declarator in node.named_children(&mut cursor) {
                if declarator.kind() != "variable_declarator" {
                    continue;
                }
                let Some(name) = child_text(declarator, "name", src) else {
                    continue;
                };
                let Some(value) = declarator.child_by_field_name("value") else {
                    continue;
                };
                let is_fn = matches!(
                    value.kind(),
                    "arrow_function" | "function_expression" | "function" | "generator_function"
                );
                if is_fn {
                    push("function", name, declarator, src, out, parent);
                }
            }
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_js_ts(child, src, out, parent);
    }
}

// ─── Python ───────────────────────────────────────────────────────────────────

fn walk_python(node: Node<'_>, src: &[u8], out: &mut Vec<ExtractedSymbol>, parent: Option<usize>) {
    fn py_docstring(node: Node<'_>, src: &[u8]) -> Option<String> {
        let body = node.child_by_field_name("body")?;
        let mut cursor = body.walk();
        let first = body.named_children(&mut cursor).next()?;
        if first.kind() != "expression_statement" {
            return None;
        }
        let mut inner_cursor = first.walk();
        let inner = first.named_children(&mut inner_cursor).next()?;
        if inner.kind() != "string" {
            return None;
        }
        Some(inner.utf8_text(src).ok()?.to_string())
    }
    let (kind, def_node) = match node.kind() {
        "function_definition" => ("function", node),
        "class_definition" => ("class", node),
        "decorated_definition" => {
            // Find the inner def / class for the actual name.
            let mut cursor = node.walk();
            let inner = node.named_children(&mut cursor).find(|c| {
                matches!(c.kind(), "function_definition" | "class_definition")
            });
            if let Some(inner) = inner {
                let k = if inner.kind() == "class_definition" {
                    "class"
                } else {
                    "function"
                };
                (k, inner)
            } else {
                ("function", node)
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                walk_python(child, src, out, parent);
            }
            return;
        }
    };
    let Some(name) = child_text(def_node, "name", src) else {
        return;
    };
    out.push(ExtractedSymbol {
        kind: kind.to_string(),
        name,
        line_start: line_start(node),
        line_end: line_end(node),
        parent_idx: parent,
        doc_comment: py_docstring(def_node, src),
    });
    let idx = Some(out.len() - 1);
    if kind == "class" {
        if let Some(body) = def_node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.named_children(&mut cursor) {
                // Methods nested in a class get parent=class.
                walk_python(child, src, out, idx);
            }
        }
    }
}

// ─── Go ───────────────────────────────────────────────────────────────────────

fn walk_go(node: Node<'_>, src: &[u8], out: &mut Vec<ExtractedSymbol>, parent: Option<usize>) {
    fn go_doc(node: Node<'_>, src: &[u8]) -> Option<String> {
        leading_doc(node, src, &["comment"], |text| text.starts_with("//"))
    }
    match node.kind() {
        "function_declaration" => {
            if let Some(name) = child_text(node, "name", src) {
                out.push(ExtractedSymbol {
                    kind: "function".to_string(),
                    name,
                    line_start: line_start(node),
                    line_end: line_end(node),
                    parent_idx: parent,
                    doc_comment: go_doc(node, src),
                });
            }
            return;
        }
        "method_declaration" => {
            if let Some(name) = child_text(node, "name", src) {
                out.push(ExtractedSymbol {
                    kind: "method".to_string(),
                    name,
                    line_start: line_start(node),
                    line_end: line_end(node),
                    parent_idx: parent,
                    doc_comment: go_doc(node, src),
                });
            }
            return;
        }
        "type_declaration" => {
            // `type X struct { ... }`, `type X interface { ... }`,
            // `type X = Y`, `type X Y` — each `type_spec` child gets
            // its own row keyed by the underlying type-form node.
            let mut cursor = node.walk();
            for spec in node.named_children(&mut cursor) {
                if spec.kind() != "type_spec" {
                    continue;
                }
                let Some(name) = child_text(spec, "name", src) else {
                    continue;
                };
                let type_kind = spec
                    .child_by_field_name("type")
                    .map_or("type", |n| n.kind());
                let kind = match type_kind {
                    "struct_type" => "struct",
                    "interface_type" => "interface",
                    _ => "type",
                };
                out.push(ExtractedSymbol {
                    kind: kind.to_string(),
                    name,
                    line_start: line_start(spec),
                    line_end: line_end(spec),
                    parent_idx: parent,
                    doc_comment: go_doc(node, src),
                });
            }
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_go(child, src, out, parent);
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{configure_pragmas, migrate};
    use rusqlite::Connection;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        configure_pragmas(&conn).unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn detect_language_matches_extensions() {
        assert_eq!(detect_language("a.rs"), Some(CodeLanguage::Rust));
        assert_eq!(detect_language("a.ts"), Some(CodeLanguage::TypeScript));
        assert_eq!(detect_language("a.tsx"), Some(CodeLanguage::Tsx));
        assert_eq!(detect_language("a.js"), Some(CodeLanguage::JavaScript));
        assert_eq!(detect_language("a.jsx"), Some(CodeLanguage::Jsx));
        assert_eq!(detect_language("a.py"), Some(CodeLanguage::Python));
        assert_eq!(detect_language("a.go"), Some(CodeLanguage::Go));
        assert_eq!(detect_language("a.mjs"), Some(CodeLanguage::JavaScript));
        assert_eq!(detect_language("a.md"), None);
        assert_eq!(detect_language("Cargo.toml"), None);
        assert_eq!(detect_language("nodot"), None);
    }

    #[test]
    fn rust_extracts_function_struct_and_methods() {
        let src = r#"
/// Doc one.
/// Doc two.
pub fn top_level() {}

pub struct Counter { value: i32 }

impl Counter {
    pub fn new() -> Self { Self { value: 0 } }
    pub fn bump(&mut self) { self.value += 1; }
}

pub trait Greet {
    fn hello(&self);
}

const MAX: u32 = 42;
"#;
        let syms = extract_symbols(CodeLanguage::Rust, src);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"top_level"));
        assert!(names.contains(&"Counter"));
        assert!(names.contains(&"new"));
        assert!(names.contains(&"bump"));
        assert!(names.contains(&"Greet"));
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"MAX"));

        // `new` / `bump` parent_idx points back to the impl row.
        let impl_idx = syms.iter().position(|s| s.kind == "impl").unwrap();
        let new_sym = syms.iter().find(|s| s.name == "new").unwrap();
        assert_eq!(new_sym.parent_idx, Some(impl_idx));

        let top = syms.iter().find(|s| s.name == "top_level").unwrap();
        assert!(top.doc_comment.as_deref().unwrap().contains("Doc one"));
    }

    #[test]
    fn typescript_extracts_class_methods_and_interfaces() {
        let src = r#"
/** JSDoc for foo. */
export function foo(): number { return 1; }

export class Bar {
  greet(): void {}
  static factory(): Bar { return new Bar(); }
}

export interface User { id: string; name: string }
export type Id = string;
export const arrowFn = (x: number) => x + 1;
"#;
        let syms = extract_symbols(CodeLanguage::TypeScript, src);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"Bar"));
        assert!(names.contains(&"greet"));
        assert!(names.contains(&"factory"));
        assert!(names.contains(&"User"));
        assert!(names.contains(&"Id"));
        assert!(names.contains(&"arrowFn"));
        let foo = syms.iter().find(|s| s.name == "foo").unwrap();
        assert!(foo.doc_comment.as_deref().unwrap().contains("JSDoc for foo"));
        let class_idx = syms.iter().position(|s| s.name == "Bar").unwrap();
        let greet = syms.iter().find(|s| s.name == "greet").unwrap();
        assert_eq!(greet.parent_idx, Some(class_idx));
    }

    #[test]
    fn python_extracts_functions_classes_and_docstrings() {
        let src = "\
def hello(name):
    \"\"\"Greet the named user.\"\"\"
    return f'hi {name}'

class Counter:
    \"\"\"Counter class.\"\"\"

    def bump(self):
        self.value += 1
";
        let syms = extract_symbols(CodeLanguage::Python, src);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"Counter"));
        assert!(names.contains(&"bump"));
        let counter_idx = syms.iter().position(|s| s.name == "Counter").unwrap();
        let bump = syms.iter().find(|s| s.name == "bump").unwrap();
        assert_eq!(bump.parent_idx, Some(counter_idx));
        let hello = syms.iter().find(|s| s.name == "hello").unwrap();
        assert!(hello.doc_comment.as_deref().unwrap().contains("Greet"));
    }

    #[test]
    fn go_extracts_funcs_methods_and_types() {
        let src = "\
package main

// Greet says hi.
func Greet(name string) string { return \"hi \" + name }

type Counter struct {
    value int
}

func (c *Counter) Bump() { c.value++ }

type Greeter interface { Greet() string }
";
        let syms = extract_symbols(CodeLanguage::Go, src);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Greet"));
        assert!(names.contains(&"Counter"));
        assert!(names.contains(&"Bump"));
        assert!(names.contains(&"Greeter"));
        let counter = syms.iter().find(|s| s.name == "Counter").unwrap();
        assert_eq!(counter.kind, "struct");
        let greeter = syms.iter().find(|s| s.name == "Greeter").unwrap();
        assert_eq!(greeter.kind, "interface");
        let greet = syms.iter().find(|s| s.name == "Greet").unwrap();
        assert!(greet.doc_comment.as_deref().unwrap().contains("Greet says hi"));
    }

    #[test]
    fn upsert_replaces_prior_rows_for_path() {
        let conn = db();
        let v1 = extract_symbols(CodeLanguage::Rust, "pub fn a() {}\npub fn b() {}\n");
        upsert_file_symbols(&conn, "src/lib.rs", CodeLanguage::Rust, &v1).unwrap();
        let initial = count_symbols(&conn).unwrap();
        assert_eq!(initial, 2);

        let v2 = extract_symbols(CodeLanguage::Rust, "pub fn a() {}\n");
        upsert_file_symbols(&conn, "src/lib.rs", CodeLanguage::Rust, &v2).unwrap();
        let after = count_symbols(&conn).unwrap();
        assert_eq!(after, 1, "stale rows from v1 should be removed");
    }

    #[test]
    fn query_filters_by_name_and_path() {
        let conn = db();
        let rust_syms = extract_symbols(CodeLanguage::Rust, "pub fn shared() {}\npub fn only_a() {}\n");
        upsert_file_symbols(&conn, "a.rs", CodeLanguage::Rust, &rust_syms).unwrap();
        let other = extract_symbols(CodeLanguage::Rust, "pub fn shared() {}\npub fn only_b() {}\n");
        upsert_file_symbols(&conn, "b.rs", CodeLanguage::Rust, &other).unwrap();

        let by_name = query_symbols(
            &conn,
            &SymbolFilter {
                name: Some("shared".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_name.len(), 2);

        let by_name_and_path = query_symbols(
            &conn,
            &SymbolFilter {
                name: Some("shared".into()),
                path: Some("a.rs".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_name_and_path.len(), 1);
        assert_eq!(by_name_and_path[0].path, "a.rs");
    }

    #[test]
    fn delete_clears_rows() {
        let conn = db();
        let syms = extract_symbols(CodeLanguage::Rust, "pub fn one() {}\n");
        upsert_file_symbols(&conn, "a.rs", CodeLanguage::Rust, &syms).unwrap();
        assert_eq!(count_symbols(&conn).unwrap(), 1);
        delete_file_symbols(&conn, "a.rs").unwrap();
        assert_eq!(count_symbols(&conn).unwrap(), 0);
    }

    #[test]
    fn parent_id_resolved_after_insert() {
        let conn = db();
        let src = r#"
impl Counter {
    pub fn new() -> Self { Self {} }
}
"#;
        let syms = extract_symbols(CodeLanguage::Rust, src);
        upsert_file_symbols(&conn, "a.rs", CodeLanguage::Rust, &syms).unwrap();
        let rows = query_symbols(
            &conn,
            &SymbolFilter {
                path: Some("a.rs".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let impl_row = rows.iter().find(|r| r.kind == "impl").unwrap();
        let new_row = rows.iter().find(|r| r.name == "new").unwrap();
        assert_eq!(new_row.parent_id, Some(impl_row.id));
    }
}
