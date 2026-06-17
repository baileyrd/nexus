//! Tree-sitter structural code search (`ast_query`) — Phase 5.2 / RFC 0005.
//!
//! Runs a [tree-sitter query] (an S-expression pattern with `@capture`s) over
//! the forge's code files of one language and returns each capture's location
//! and text. Unlike the FTS / regex searches this matches *structure*, so the
//! agent can ask things like "every function named `foo`" or "all `await`
//! expressions" rather than guessing at text.
//!
//! Dependency-light by design: it reuses the tree-sitter grammars already wired
//! for the symbol index ([`crate::code_index`]) and adds no parser of its own.
//! omp's `ast_grep` used a `$VAR` pattern dialect; this is the tree-sitter-query
//! equivalent built on grammars Nexus already ships.
//!
//! [tree-sitter query]: https://tree-sitter.github.io/tree-sitter/using-parsers#query-syntax

use std::path::{Path, PathBuf};

use streaming_iterator::StreamingIterator;
use thiserror::Error;
use tree_sitter::{Parser, Query, QueryCursor};

use crate::code_index::{detect_language, language_from_label, ts_language};
use crate::error::StorageError;
use crate::find_replace::collect_text_files;
use crate::ipc::{StorageAstQueryArgs, StorageAstQueryMatch, StorageAstQueryResult};

/// Default cap on returned matches.
pub const DEFAULT_AST_MAX_RESULTS: usize = 100;
/// Longest captured-text snippet returned per match (bytes), UTF-8-safe.
pub const MAX_SNIPPET_BYTES: usize = 240;

/// What can go wrong running an `ast_query`.
#[derive(Debug, Error)]
pub enum AstQueryError {
    /// The `language` label was not one of the supported grammars.
    #[error(
        "unknown language '{0}' (supported: rust, typescript, tsx, javascript, jsx, python, go)"
    )]
    UnknownLanguage(String),
    /// The tree-sitter query failed to compile against the grammar.
    #[error("invalid tree-sitter query: {0}")]
    BadQuery(String),
    /// A storage / filesystem error while walking the forge.
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Run a tree-sitter query over the forge's code files for one language.
///
/// # Errors
///
/// [`AstQueryError::UnknownLanguage`] for an unsupported `language`,
/// [`AstQueryError::BadQuery`] for a query that does not compile, or
/// [`AstQueryError::Storage`] if the forge walk fails.
pub fn ast_query(
    forge_root: &Path,
    args: &StorageAstQueryArgs,
) -> Result<StorageAstQueryResult, AstQueryError> {
    let language =
        language_from_label(&args.language).ok_or_else(|| AstQueryError::UnknownLanguage(args.language.clone()))?;
    let lang = ts_language(language);
    let query = Query::new(&lang, &args.query).map_err(|e| AstQueryError::BadQuery(e.to_string()))?;
    let capture_names = query.capture_names();

    let max = args
        .max_results
        .map_or(DEFAULT_AST_MAX_RESULTS, |n| n as usize)
        .max(1);

    let mut parser = Parser::new();
    if parser.set_language(&lang).is_err() {
        return Err(AstQueryError::BadQuery(
            "failed to load grammar for language".to_string(),
        ));
    }

    let files = candidate_files(forge_root, args)?;
    let mut matches = Vec::new();
    let mut truncated = false;

    'files: for rel in files {
        let rel_str = rel.to_string_lossy();
        if detect_language(&rel_str) != Some(language) {
            continue;
        }
        if let Some(scope) = &args.path {
            if !rel_str.starts_with(scope.as_str()) {
                continue;
            }
        }
        let Ok(bytes) = std::fs::read(forge_root.join(&rel)) else {
            continue;
        };
        let Ok(source) = String::from_utf8(bytes) else {
            continue;
        };
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };

        let src = source.as_bytes();
        let mut cursor = QueryCursor::new();
        let mut it = cursor.matches(&query, tree.root_node(), src);
        while let Some(m) = it.next() {
            for cap in m.captures {
                let node = cap.node;
                let name = capture_names
                    .get(cap.index as usize)
                    .copied()
                    .unwrap_or("");
                let text = node.utf8_text(src).unwrap_or("");
                matches.push(StorageAstQueryMatch {
                    path: rel_str.to_string(),
                    line: u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX),
                    capture: name.to_string(),
                    text: snippet(text),
                });
                if matches.len() >= max {
                    truncated = true;
                    break 'files;
                }
            }
        }
    }

    Ok(StorageAstQueryResult { matches, truncated })
}

/// Files to consider: a single file when `path` names one, otherwise every
/// text file in the forge (language/scope filtering happens in the loop).
fn candidate_files(forge_root: &Path, args: &StorageAstQueryArgs) -> Result<Vec<PathBuf>, StorageError> {
    if let Some(scope) = &args.path {
        let candidate = PathBuf::from(scope);
        if forge_root.join(&candidate).is_file() {
            return Ok(vec![candidate]);
        }
    }
    collect_text_files(forge_root)
}

/// UTF-8-safe truncation of a captured snippet to [`MAX_SNIPPET_BYTES`].
fn snippet(text: &str) -> String {
    if text.len() <= MAX_SNIPPET_BYTES {
        return text.to_string();
    }
    let mut end = MAX_SNIPPET_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageEngine;
    use tempfile::TempDir;

    fn forge() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = StorageEngine::init(dir.path()).expect("init");
        engine
            .write_file(
                "notes/lib.rs",
                b"pub fn alpha() {}\npub fn beta() {}\nstruct S;\n",
            )
            .expect("write");
        let root = dir.path().to_path_buf();
        (dir, root)
    }

    fn args(language: &str, query: &str) -> StorageAstQueryArgs {
        StorageAstQueryArgs {
            language: language.to_string(),
            query: query.to_string(),
            path: None,
            max_results: None,
        }
    }

    #[test]
    fn matches_function_names() {
        let (_d, root) = forge();
        let q = "(function_item name: (identifier) @fn)";
        let res = ast_query(&root, &args("rust", q)).expect("query");
        let names: Vec<&str> = res.matches.iter().map(|m| m.text.as_str()).collect();
        assert!(names.contains(&"alpha"), "got {names:?}");
        assert!(names.contains(&"beta"), "got {names:?}");
        assert!(res.matches.iter().all(|m| m.capture == "fn"));
        assert!(res.matches.iter().all(|m| m.path == "notes/lib.rs"));
    }

    #[test]
    fn unknown_language_errors() {
        let (_d, root) = forge();
        let err = ast_query(&root, &args("cobol", "(x)")).unwrap_err();
        assert!(matches!(err, AstQueryError::UnknownLanguage(_)));
    }

    #[test]
    fn bad_query_errors() {
        let (_d, root) = forge();
        let err = ast_query(&root, &args("rust", "(this is not valid")).unwrap_err();
        assert!(matches!(err, AstQueryError::BadQuery(_)));
    }

    #[test]
    fn max_results_truncates() {
        let (_d, root) = forge();
        let mut a = args("rust", "(function_item name: (identifier) @fn)");
        a.max_results = Some(1);
        let res = ast_query(&root, &a).expect("query");
        assert_eq!(res.matches.len(), 1);
        assert!(res.truncated);
    }
}
