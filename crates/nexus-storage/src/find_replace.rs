//! BL-078 — multi-file find / replace.
//!
//! # Role
//!
//! Walks the forge tree, scans every non-ignored UTF-8 text file for
//! a query (literal, regex, case-sensitive / -insensitive, whole-word
//! optional), and returns the matches grouped by file with one line
//! of leading + trailing context per hit. The replace path applies a
//! caller-confirmed substitution against the same matcher and writes
//! changed files back through the regular file write path so the
//! storage index + bus events stay consistent.
//!
//! # Microkernel fit
//!
//! Plain library — same shape as [`reconcile`]. The IPC layer in
//! [`crate::core_plugin`] passes a `&Path` for the forge root and
//! shapes the return values into JSON; this module knows nothing
//! about IPC dispatch.
//!
//! # What this is NOT
//!
//! - A Tantivy front-end. The existing FTS index returns block-level
//!   BM25 matches, which doesn't carry line numbers and doesn't honor
//!   case / regex / whole-word. The cost we pay is reading every
//!   non-ignored text file once per search; in exchange we get exact
//!   semantics. A future optimisation can prune candidate files
//!   through Tantivy first when `is_regex == false`, but that's a
//!   speedup over correctness, not a precondition.
//! - A binary-file search. Files that don't decode as UTF-8 are
//!   skipped silently. The user's "find in files" mental model is
//!   text-only; surfacing binary hits would just be noise.
//! - An incremental / streaming surface. The handler returns the
//!   full result set in one IPC round-trip. For very large forges,
//!   the caller bounds the cost via `max_files` / `max_results`. A
//!   true streaming surface is a future revision keyed on a real
//!   pain point.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::watcher::should_ignore;
use crate::StorageError;

/// Default cap on the number of files returned. Picked to keep the
/// IPC payload bounded under typical forge sizes; the shell UI can
/// page beyond this by tightening the query.
pub const DEFAULT_MAX_FILES: usize = 200;

/// Default cap on the *total* number of line hits across all files.
/// Stops a query like `the` from dumping the entire forge.
pub const DEFAULT_MAX_RESULTS: usize = 1_000;

/// BL-078 — args for [`find_in_files`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct FindInFilesArgs {
    /// Query string. When `is_regex` is `false`, treated as a literal
    /// substring. When `true`, compiled via `regex_lite::Regex`. An
    /// empty / whitespace-only query returns an empty result set
    /// rather than every line in the forge.
    pub query: String,
    /// Treat `query` as a regex pattern. The matcher uses
    /// `regex_lite` — same engine as the editor's Find / Replace
    /// dialog and the cross-session terminal search.
    #[serde(default)]
    pub is_regex: bool,
    /// `false` (the default) folds case before matching. `true`
    /// preserves case both ways.
    #[serde(default)]
    pub case_sensitive: bool,
    /// Constrain matches to whole words (i.e. wrap the query in
    /// `\b…\b`). Combines with `is_regex` — when both are true the
    /// caller's pattern is wrapped, not parsed for word boundaries
    /// already inside it. Whole-word semantics use the regex `\b`
    /// definition (transition between `\w` and non-`\w`); the
    /// literal-path implementation derives the same boundary check
    /// inline.
    #[serde(default)]
    pub whole_word: bool,
    /// Cap the number of files in the response. `None` defaults to
    /// [`DEFAULT_MAX_FILES`].
    #[serde(default)]
    pub max_files: Option<u32>,
    /// Cap the *total* line hits across all files. `None` defaults
    /// to [`DEFAULT_MAX_RESULTS`].
    #[serde(default)]
    pub max_results: Option<u32>,
}

/// One file's hits. Files with zero hits are not included in the
/// response at all; the result list is "every file that matched".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct FileMatches {
    /// Forge-relative path of the file.
    pub relpath: String,
    /// Each match in the file, ordered by line number ascending.
    pub hits: Vec<LineMatch>,
}

/// One line within a file that matched the query, with one line of
/// surrounding context above and below to give the user enough to
/// recognise the hit without re-opening the file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct LineMatch {
    /// 1-based line number (matches editor / CLI conventions).
    pub line: u32,
    /// 0-based byte column of the first character of the match
    /// within the line.
    pub column: u32,
    /// Length of the match in *bytes*. The shell UI uses this with
    /// `column` to render a highlight overlay on the matched span.
    pub length: u32,
    /// The literal line that matched (no trailing newline).
    pub text: String,
    /// One line of leading context (`None` when the match is on the
    /// first line of the file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// One line of trailing context (`None` when the match is on
    /// the last line).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// BL-078 — args for [`replace_in_files`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReplaceInFilesArgs {
    /// Same matcher as [`FindInFilesArgs::query`] — keep both surfaces
    /// in sync so the user sees the same hits they're about to mutate.
    pub query: String,
    /// Replacement string. When `is_regex == true`, capture-group
    /// references (`$1`, `${name}`) expand against the regex match;
    /// otherwise the replacement is a literal.
    pub replacement: String,
    /// Treat `query` as a regex pattern. See [`FindInFilesArgs::is_regex`].
    #[serde(default)]
    pub is_regex: bool,
    /// `false` (the default) folds case before matching. See
    /// [`FindInFilesArgs::case_sensitive`].
    #[serde(default)]
    pub case_sensitive: bool,
    /// Constrain matches to whole words. See
    /// [`FindInFilesArgs::whole_word`].
    #[serde(default)]
    pub whole_word: bool,
    /// Optional list of forge-relative paths to confine the
    /// replacement to. `None` (or empty) replaces in every matching
    /// file. The shell UI's "apply per file" flow passes a single
    /// path here; the "apply all" flow passes the full set or
    /// omits the field.
    #[serde(default)]
    pub files: Option<Vec<String>>,
}

/// BL-078 — return type for [`replace_in_files`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReplaceReport {
    /// Number of files where at least one byte changed.
    pub files_changed: u32,
    /// Total number of substring / regex replacements applied
    /// across every changed file.
    pub replacements_applied: u32,
    /// Per-file errors that didn't abort the whole batch (e.g. a
    /// permission-denied write on one file out of many). Other
    /// files in the batch still go through; the caller renders this
    /// list to the user.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ReplaceError>,
}

/// One per-file failure inside a [`ReplaceReport`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReplaceError {
    /// Forge-relative path of the file that failed.
    pub relpath: String,
    /// One-line error message.
    pub message: String,
}

/// Internal matcher built once per call. Captures the query +
/// modifiers so the per-line scan loop doesn't re-parse the regex
/// or rebuild the casefold state.
enum Matcher {
    Literal {
        needle: String,
        case_sensitive: bool,
        whole_word: bool,
    },
    Regex(regex_lite::Regex),
}

impl Matcher {
    /// Build the matcher from the query / modifier set. Returns
    /// `None` for empty / whitespace-only queries (the caller
    /// short-circuits to an empty result).
    fn build(
        query: &str,
        is_regex: bool,
        case_sensitive: bool,
        whole_word: bool,
    ) -> Result<Option<Self>, StorageError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if is_regex {
            // Compose flags + optional whole-word boundary wrap. The
            // `(?i)` inline flag is the right knob in regex_lite —
            // the crate doesn't expose a builder.
            let mut pattern = String::new();
            if !case_sensitive {
                pattern.push_str("(?i)");
            }
            if whole_word {
                pattern.push_str(r"\b(?:");
                pattern.push_str(query);
                pattern.push_str(r")\b");
            } else {
                pattern.push_str(query);
            }
            let re = regex_lite::Regex::new(&pattern).map_err(|e| {
                StorageError::ConfigInvalid(format!("invalid regex '{query}': {e}"))
            })?;
            Ok(Some(Self::Regex(re)))
        } else {
            Ok(Some(Self::Literal {
                needle: query.to_string(),
                case_sensitive,
                whole_word,
            }))
        }
    }

    /// Scan one line for matches. Returns `(byte_column, byte_length)`
    /// pairs in ascending column order. The literal path keeps the
    /// allocation cost predictable; the regex path delegates to
    /// `regex_lite::find_iter`.
    fn find_in_line(&self, line: &str) -> Vec<(u32, u32)> {
        let mut hits = Vec::new();
        match self {
            Matcher::Literal {
                needle,
                case_sensitive,
                whole_word,
            } => {
                if needle.is_empty() {
                    return hits;
                }
                let haystack: std::borrow::Cow<'_, str> = if *case_sensitive {
                    line.into()
                } else {
                    line.to_lowercase().into()
                };
                let folded_needle: std::borrow::Cow<'_, str> = if *case_sensitive {
                    needle.into()
                } else {
                    needle.to_lowercase().into()
                };
                let mut start = 0usize;
                while let Some(idx) = haystack[start..].find(folded_needle.as_ref()) {
                    let abs = start + idx;
                    let len = folded_needle.len();
                    if !*whole_word || is_whole_word_boundary(line, abs, len) {
                        hits.push((
                            u32::try_from(abs).unwrap_or(u32::MAX),
                            u32::try_from(len).unwrap_or(u32::MAX),
                        ));
                    }
                    start = abs + len.max(1);
                }
            }
            Matcher::Regex(re) => {
                for m in re.find_iter(line) {
                    let len = m.end() - m.start();
                    hits.push((
                        u32::try_from(m.start()).unwrap_or(u32::MAX),
                        u32::try_from(len).unwrap_or(u32::MAX),
                    ));
                }
            }
        }
        hits
    }

    /// Apply the matcher to `text` and return `(new_text, count)`,
    /// where `count` is the number of substitutions applied. The
    /// regex path uses `regex_lite::Regex::replace_all` (with
    /// capture-group expansion against `replacement`); the literal
    /// path does an explicit walk so we can count replacements.
    fn replace_all(&self, text: &str, replacement: &str) -> (String, u32) {
        match self {
            Matcher::Literal {
                needle,
                case_sensitive,
                whole_word,
            } => {
                if needle.is_empty() {
                    return (text.to_string(), 0);
                }
                let mut out = String::with_capacity(text.len());
                let mut count = 0u32;
                // Per-line replacement keeps line-level whole-word
                // semantics intact — `\b` boundaries are computed
                // against the same line, which is the natural unit
                // a user thinks in.
                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    let hits = Matcher::Literal {
                        needle: needle.clone(),
                        case_sensitive: *case_sensitive,
                        whole_word: *whole_word,
                    }
                    .find_in_line(line);
                    let mut last = 0usize;
                    for (col, len) in hits {
                        let col = col as usize;
                        let len = len as usize;
                        out.push_str(&line[last..col]);
                        out.push_str(replacement);
                        last = col + len;
                        count = count.saturating_add(1);
                    }
                    out.push_str(&line[last..]);
                }
                (out, count)
            }
            Matcher::Regex(re) => {
                // `replace_all` doesn't tell us the count directly;
                // probe via `find_iter` first.
                let count = u32::try_from(re.find_iter(text).count()).unwrap_or(u32::MAX);
                let new = re.replace_all(text, replacement).into_owned();
                (new, count)
            }
        }
    }
}

/// Word-boundary check used by the literal whole-word path. A match
/// at `[col, col+len)` qualifies when:
///   - the byte before is non-word (or the match starts the line), AND
///   - the byte after is non-word (or the match ends the line).
///
/// "Word" follows ASCII `\w` semantics: alphanumeric or underscore.
/// Matches the literal-search semantics of every editor we care
/// about (VS Code, Sublime, Vim).
fn is_whole_word_boundary(line: &str, col: usize, len: usize) -> bool {
    let bytes = line.as_bytes();
    let before_ok = if col == 0 {
        true
    } else {
        !is_word_byte(bytes[col - 1])
    };
    let after_idx = col + len;
    let after_ok = if after_idx >= bytes.len() {
        true
    } else {
        !is_word_byte(bytes[after_idx])
    };
    before_ok && after_ok
}

const fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// BL-078 — search every non-ignored UTF-8 file under `forge_root`.
/// Files that don't decode as UTF-8 are skipped silently; binary
/// files would otherwise show up as garbage hits.
///
/// Returns `Vec<FileMatches>` ordered by relpath ascending so the
/// shell UI can render the result tree without a client-side sort.
///
/// # Errors
/// - [`StorageError::Other`] if `args.query` is a malformed regex.
/// - [`StorageError::Io`] if walking the forge root fails.
pub fn find_in_files(
    forge_root: &Path,
    args: &FindInFilesArgs,
) -> Result<Vec<FileMatches>, StorageError> {
    let Some(matcher) = Matcher::build(
        &args.query,
        args.is_regex,
        args.case_sensitive,
        args.whole_word,
    )?
    else {
        return Ok(Vec::new());
    };
    let max_files = args
        .max_files
        .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
        .unwrap_or(DEFAULT_MAX_FILES)
        .max(1);
    let max_results = args
        .max_results
        .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .max(1);

    let files = collect_text_files(forge_root)?;
    let mut out: Vec<FileMatches> = Vec::new();
    let mut total_hits = 0usize;
    for relpath in files {
        if out.len() >= max_files || total_hits >= max_results {
            break;
        }
        let abs = forge_root.join(&relpath);
        let bytes = match std::fs::read(&abs) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let lines: Vec<&str> = content.lines().collect();
        let mut hits: Vec<LineMatch> = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            let line_hits = matcher.find_in_line(line);
            for (col, len) in line_hits {
                hits.push(LineMatch {
                    line: u32::try_from(idx + 1).unwrap_or(u32::MAX),
                    column: col,
                    length: len,
                    text: (*line).to_string(),
                    before: idx
                        .checked_sub(1)
                        .and_then(|prev| lines.get(prev))
                        .map(|s| (*s).to_string()),
                    after: lines.get(idx + 1).map(|s| (*s).to_string()),
                });
                total_hits += 1;
                if total_hits >= max_results {
                    break;
                }
            }
            if total_hits >= max_results {
                break;
            }
        }
        if !hits.is_empty() {
            out.push(FileMatches {
                relpath: relpath.to_string_lossy().into_owned(),
                hits,
            });
        }
    }
    out.sort_by(|a, b| a.relpath.cmp(&b.relpath));
    Ok(out)
}

/// BL-078 — apply `replacement` against every match of `query` in
/// the files under `forge_root`. Files outside `args.files` (when
/// `Some`) are skipped. Files that don't change are skipped
/// (zero-replacement count).
///
/// Per-file errors are collected into the [`ReplaceReport`] rather
/// than aborting the batch; one bad file shouldn't prevent the
/// remaining replacements.
///
/// # Errors
/// - [`StorageError::Other`] if `args.query` is a malformed regex.
/// - [`StorageError::Io`] if walking the forge root fails.
pub fn replace_in_files(
    forge_root: &Path,
    args: &ReplaceInFilesArgs,
) -> Result<ReplaceReport, StorageError> {
    let Some(matcher) = Matcher::build(
        &args.query,
        args.is_regex,
        args.case_sensitive,
        args.whole_word,
    )?
    else {
        return Ok(ReplaceReport::default());
    };

    let restrict: Option<std::collections::HashSet<&str>> = args
        .files
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());
    let candidates = collect_text_files(forge_root)?;
    let mut report = ReplaceReport::default();
    for relpath in candidates {
        let relstr = relpath.to_string_lossy();
        if let Some(set) = &restrict {
            if !set.contains(relstr.as_ref()) {
                continue;
            }
        }
        let abs = forge_root.join(&relpath);
        let bytes = match std::fs::read(&abs) {
            Ok(b) => b,
            Err(e) => {
                report.errors.push(ReplaceError {
                    relpath: relstr.into_owned(),
                    message: format!("read failed: {e}"),
                });
                continue;
            }
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            // Binary file ended up in the candidate list (rare; the
            // walker is text-only-best-effort). Skip silently.
            continue;
        };
        let (new_content, count) = matcher.replace_all(content, &args.replacement);
        if count == 0 || new_content == content {
            continue;
        }
        if let Err(e) = std::fs::write(&abs, new_content.as_bytes()) {
            report.errors.push(ReplaceError {
                relpath: relstr.into_owned(),
                message: format!("write failed: {e}"),
            });
            continue;
        }
        report.files_changed = report.files_changed.saturating_add(1);
        report.replacements_applied = report.replacements_applied.saturating_add(count);
    }
    Ok(report)
}

/// Walk `forge_root` and return forge-relative paths of every
/// non-ignored regular file. Symlinks are skipped (consistent with
/// BL-082's reconcile policy). Order is the OS's `read_dir` order
/// per directory; the caller sorts the final result.
pub(crate) fn collect_text_files(forge_root: &Path) -> Result<Vec<PathBuf>, StorageError> {
    let mut out = Vec::new();
    walk_into(forge_root, forge_root, &mut out)?;
    Ok(out)
}

fn walk_into(dir: &Path, forge_root: &Path, out: &mut Vec<PathBuf>) -> Result<(), StorageError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if should_ignore(&path) {
            continue;
        }
        let meta = match entry.file_type() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_symlink() {
            // Same skip rule as `reconcile::scan_directory`.
            continue;
        }
        if meta.is_dir() {
            walk_into(&path, forge_root, out)?;
        } else if meta.is_file() {
            if let Ok(rel) = path.strip_prefix(forge_root) {
                out.push(rel.to_path_buf());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(forge: &Path, rel: &str, contents: &str) {
        let abs = forge.join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&abs, contents).unwrap();
    }

    fn args(query: &str) -> FindInFilesArgs {
        FindInFilesArgs {
            query: query.into(),
            is_regex: false,
            case_sensitive: false,
            whole_word: false,
            max_files: None,
            max_results: None,
        }
    }

    #[test]
    fn empty_query_returns_empty() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "hello world");
        let hits = find_in_files(forge, &args("")).unwrap();
        assert!(hits.is_empty());
        let hits = find_in_files(forge, &args("   ")).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn literal_match_picks_up_every_hit_with_context() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(
            forge,
            "notes.md",
            "first line\nthe answer is 42\nlast line\n",
        );
        let hits = find_in_files(forge, &args("answer")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].relpath, "notes.md");
        assert_eq!(hits[0].hits.len(), 1);
        let h = &hits[0].hits[0];
        assert_eq!(h.line, 2);
        assert_eq!(h.column, 4);
        assert_eq!(h.length, 6);
        assert_eq!(h.text, "the answer is 42");
        assert_eq!(h.before.as_deref(), Some("first line"));
        assert_eq!(h.after.as_deref(), Some("last line"));
    }

    #[test]
    fn case_insensitive_default_matches_mixed_case() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "Error and ERROR and error");
        let hits = find_in_files(forge, &args("ERROR")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].hits.len(), 3);
    }

    #[test]
    fn case_sensitive_only_matches_exact_case() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "Error and ERROR and error");
        let mut a = args("ERROR");
        a.case_sensitive = true;
        let hits = find_in_files(forge, &a).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].hits.len(), 1);
    }

    #[test]
    fn whole_word_excludes_substrings() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "test testing tested");
        let mut a = args("test");
        a.whole_word = true;
        let hits = find_in_files(forge, &a).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].hits.len(), 1);
        assert_eq!(hits[0].hits[0].column, 0);
    }

    #[test]
    fn regex_pattern_matches() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "port 3000\nport 8080\nplain text");
        let mut a = args(r"port \d+");
        a.is_regex = true;
        let hits = find_in_files(forge, &a).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].hits.len(), 2);
    }

    #[test]
    fn regex_invalid_pattern_surfaces_error() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "anything");
        let mut a = args("(unclosed");
        a.is_regex = true;
        let err = find_in_files(forge, &a).unwrap_err();
        match err {
            StorageError::ConfigInvalid(msg) => {
                assert!(msg.contains("invalid regex"), "{msg}")
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn results_grouped_per_file_sorted_by_relpath() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "z.md", "match here");
        write(forge, "a.md", "match here too");
        write(forge, "sub/m.md", "and a match");
        let hits = find_in_files(forge, &args("match")).unwrap();
        let paths: Vec<&str> = hits.iter().map(|f| f.relpath.as_str()).collect();
        // Walked-relative paths use forward slashes on Unix; on
        // Windows the join would land `sub\m.md` — the test relies
        // on Unix-only path semantics, like the rest of the
        // storage suite.
        assert_eq!(paths, vec!["a.md", "sub/m.md", "z.md"]);
    }

    #[test]
    fn binary_file_skipped_silently() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        // Invalid UTF-8 bytes — the matcher must not panic and the
        // file shouldn't appear in the result list.
        fs::write(forge.join("blob.bin"), [0xff, 0xfe, 0xfd, b'\n']).unwrap();
        write(forge, "a.md", "match here");
        let hits = find_in_files(forge, &args("match")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].relpath, "a.md");
    }

    #[test]
    fn ignored_dot_dirs_excluded() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, ".forge/junk.md", "hidden match");
        write(forge, ".git/HEAD", "another hidden match");
        write(forge, "real.md", "visible match");
        let hits = find_in_files(forge, &args("match")).unwrap();
        let paths: Vec<&str> = hits.iter().map(|f| f.relpath.as_str()).collect();
        assert_eq!(paths, vec!["real.md"]);
    }

    #[test]
    fn max_files_caps_response_size() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        for i in 0..5 {
            write(forge, &format!("f{i}.md"), "match here");
        }
        let mut a = args("match");
        a.max_files = Some(2);
        let hits = find_in_files(forge, &a).unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn max_results_caps_total_hits() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "match\nmatch\nmatch\nmatch\nmatch");
        let mut a = args("match");
        a.max_results = Some(3);
        let hits = find_in_files(forge, &a).unwrap();
        let total: usize = hits.iter().map(|f| f.hits.len()).sum();
        assert_eq!(total, 3);
    }

    #[test]
    fn replace_in_files_substitutes_and_writes_back() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "alpha beta alpha\ngamma");
        let report = replace_in_files(
            forge,
            &ReplaceInFilesArgs {
                query: "alpha".into(),
                replacement: "ALPHA".into(),
                is_regex: false,
                case_sensitive: false,
                whole_word: false,
                files: None,
            },
        )
        .unwrap();
        assert_eq!(report.files_changed, 1);
        assert_eq!(report.replacements_applied, 2);
        let body = fs::read_to_string(forge.join("a.md")).unwrap();
        assert_eq!(body, "ALPHA beta ALPHA\ngamma");
    }

    #[test]
    fn replace_in_files_restricts_to_files_arg() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "match");
        write(forge, "b.md", "match");
        let report = replace_in_files(
            forge,
            &ReplaceInFilesArgs {
                query: "match".into(),
                replacement: "MATCH".into(),
                is_regex: false,
                case_sensitive: false,
                whole_word: false,
                files: Some(vec!["a.md".into()]),
            },
        )
        .unwrap();
        assert_eq!(report.files_changed, 1);
        assert_eq!(report.replacements_applied, 1);
        // Only `a.md` changed; `b.md` is untouched.
        assert_eq!(fs::read_to_string(forge.join("a.md")).unwrap(), "MATCH");
        assert_eq!(fs::read_to_string(forge.join("b.md")).unwrap(), "match");
    }

    #[test]
    fn replace_in_files_regex_with_capture_group() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "id-123 and id-456");
        let report = replace_in_files(
            forge,
            &ReplaceInFilesArgs {
                query: r"id-(\d+)".into(),
                replacement: r"ID($1)".into(),
                is_regex: true,
                case_sensitive: false,
                whole_word: false,
                files: None,
            },
        )
        .unwrap();
        assert_eq!(report.replacements_applied, 2);
        let body = fs::read_to_string(forge.join("a.md")).unwrap();
        assert_eq!(body, "ID(123) and ID(456)");
    }

    #[test]
    fn replace_in_files_no_matches_leaves_files_alone() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "no hits here");
        let report = replace_in_files(
            forge,
            &ReplaceInFilesArgs {
                query: "missing".into(),
                replacement: "nope".into(),
                is_regex: false,
                case_sensitive: false,
                whole_word: false,
                files: None,
            },
        )
        .unwrap();
        assert_eq!(report.files_changed, 0);
        assert_eq!(report.replacements_applied, 0);
        assert_eq!(
            fs::read_to_string(forge.join("a.md")).unwrap(),
            "no hits here"
        );
    }

    #[test]
    fn whole_word_replace_does_not_corrupt_substrings() {
        let tmp = tempdir().unwrap();
        let forge = tmp.path();
        write(forge, "a.md", "test testing tested");
        let report = replace_in_files(
            forge,
            &ReplaceInFilesArgs {
                query: "test".into(),
                replacement: "TEST".into(),
                is_regex: false,
                case_sensitive: false,
                whole_word: true,
                files: None,
            },
        )
        .unwrap();
        assert_eq!(report.replacements_applied, 1);
        assert_eq!(
            fs::read_to_string(forge.join("a.md")).unwrap(),
            "TEST testing tested",
        );
    }
}
