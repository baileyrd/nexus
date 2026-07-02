//! C28 (#381) — path- and note-scoped AI exclusion.
//!
//! Two user-facing controls keep a note's content away from AI
//! providers (embedding, RAG retrieval, enrichment):
//!
//!   1. **`.aiignore`** at the forge root — one pattern per line,
//!      `#` comments. Supported subset (documented, deliberately
//!      simple; no `!` negation):
//!        - trailing `/` — excludes the whole subtree
//!          (`private/` matches `private/a.md`, `private/x/y.md`),
//!        - `*` matches any characters **including** `/`,
//!          `?` matches one character,
//!        - a pattern without `/` also matches against the basename
//!          (`secrets.md` matches `any/dir/secrets.md`).
//!   2. **Frontmatter** — `ai: exclude` (or `ai_exclude: true`) in a
//!      note's YAML head excludes just that note.
//!
//! Enforcement points: the indexing daemon skips (and reaps existing
//! vectors for) excluded files, `rag::retrieve` pattern-filters hits
//! as defense-in-depth, and `enrich_file` refuses outright. The
//! `.aiignore` content is fetched through `com.nexus.storage::read_file`
//! (invariant 3 — no direct fs access from a service crate) and cached
//! for [`CACHE_TTL`]; a frontmatter flip is caught by the daemon on the
//! file's own change event.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use nexus_kernel::{Ipc as _, KernelPluginContext};

use crate::indexing_daemon::STORAGE_PLUGIN_ID;

/// Forge-root file holding the exclusion patterns.
pub const AIIGNORE_FILE: &str = ".aiignore";

/// How long a loaded `.aiignore` is reused before re-reading.
const CACHE_TTL: Duration = Duration::from_secs(5);

/// Per-IPC timeout for the tiny reads this module performs.
const IPC_TIMEOUT: Duration = Duration::from_secs(5);

/// Parse `.aiignore` text into patterns (comments / blanks dropped).
#[must_use]
pub fn parse_patterns(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// `*`/`?` wildcard match; `*` spans path separators.
fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Does `relpath` (forge-relative, `/`-separated) match any pattern?
#[must_use]
pub fn path_excluded(patterns: &[String], relpath: &str) -> bool {
    let relpath = relpath.trim_start_matches("./");
    let basename = relpath.rsplit('/').next().unwrap_or(relpath);
    for pattern in patterns {
        if let Some(dir) = pattern.strip_suffix('/') {
            let dir = dir.trim_start_matches("./");
            if relpath.starts_with(&format!("{dir}/")) || relpath == dir {
                return true;
            }
            continue;
        }
        if wildcard(pattern, relpath) {
            return true;
        }
        if !pattern.contains('/') && wildcard(pattern, basename) {
            return true;
        }
    }
    false
}

/// Does a `read_frontmatter` reply opt the note out of AI? Accepts
/// `ai: exclude` / `ai: excluded` (case-insensitive) and
/// `ai_exclude: true`.
#[must_use]
pub fn frontmatter_excludes(reply: &serde_json::Value) -> bool {
    let Some(fields) = reply.get("fields").and_then(serde_json::Value::as_object) else {
        return false;
    };
    if let Some(v) = fields.get("ai").and_then(serde_json::Value::as_str) {
        let v = v.trim().to_lowercase();
        if v == "exclude" || v == "excluded" {
            return true;
        }
    }
    if let Some(v) = fields.get("ai_exclude").and_then(serde_json::Value::as_str) {
        if v.trim().eq_ignore_ascii_case("true") {
            return true;
        }
    }
    false
}

type PatternCache = Mutex<Option<(Instant, Vec<String>)>>;

fn cache() -> &'static PatternCache {
    static CACHE: OnceLock<PatternCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Test seam / explicit refresh: drop the cached `.aiignore` patterns.
pub fn invalidate_pattern_cache() {
    if let Ok(mut guard) = cache().lock() {
        *guard = None;
    }
}

/// Load (TTL-cached) `.aiignore` patterns via storage IPC. A missing
/// or unreadable file yields no patterns.
pub async fn cached_patterns(ctx: &KernelPluginContext) -> Vec<String> {
    if let Ok(guard) = cache().lock() {
        if let Some((at, patterns)) = guard.as_ref() {
            if at.elapsed() < CACHE_TTL {
                return patterns.clone();
            }
        }
    }
    let reply: Result<serde_json::Value, _> = ctx
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "read_file",
            serde_json::json!({ "path": AIIGNORE_FILE }),
            IPC_TIMEOUT,
        )
        .await;
    let patterns = match reply {
        Ok(reply) => reply
            .get("bytes")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                    .collect();
                String::from_utf8_lossy(&bytes).into_owned()
            })
            .map(|text| parse_patterns(&text))
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    if let Ok(mut guard) = cache().lock() {
        *guard = Some((Instant::now(), patterns.clone()));
    }
    patterns
}

/// Full exclusion check for one note: `.aiignore` patterns, then the
/// note's own frontmatter. IPC failures fail open (not excluded) —
/// a missing file or unreadable frontmatter must not wedge indexing.
pub async fn is_excluded(ctx: &KernelPluginContext, relpath: &str) -> bool {
    let patterns = cached_patterns(ctx).await;
    if path_excluded(&patterns, relpath) {
        return true;
    }
    match ctx
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "read_frontmatter",
            serde_json::json!({ "path": relpath }),
            IPC_TIMEOUT,
        )
        .await
    {
        Ok(reply) => frontmatter_excludes(&reply),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_comments_and_blanks() {
        let p = parse_patterns("# private stuff\n\nprivate/\n  journal-*.md  \n");
        assert_eq!(p, vec!["private/".to_string(), "journal-*.md".to_string()]);
    }

    #[test]
    fn dir_patterns_exclude_the_subtree() {
        let p = vec!["private/".to_string()];
        assert!(path_excluded(&p, "private/a.md"));
        assert!(path_excluded(&p, "private/deep/b.md"));
        assert!(path_excluded(&p, "private"));
        assert!(!path_excluded(&p, "public/a.md"));
        assert!(!path_excluded(&p, "private-notes/a.md"));
    }

    #[test]
    fn wildcards_match_within_and_across_dirs() {
        let p = vec!["journal-*.md".to_string(), "notes/*/secret?.md".to_string()];
        assert!(path_excluded(&p, "journal-2026.md"));
        assert!(path_excluded(&p, "daily/journal-01.md")); // basename match
        assert!(path_excluded(&p, "notes/x/secret1.md"));
        assert!(path_excluded(&p, "notes/a/b/secretz.md")); // * spans /
        assert!(!path_excluded(&p, "notes/x/secrets-long.md"));
    }

    #[test]
    fn exact_and_basename_matches() {
        let p = vec!["secrets.md".to_string(), "work/hr.md".to_string()];
        assert!(path_excluded(&p, "secrets.md"));
        assert!(path_excluded(&p, "any/dir/secrets.md"));
        assert!(path_excluded(&p, "work/hr.md"));
        assert!(!path_excluded(&p, "other/hr.md"));
    }

    #[test]
    fn frontmatter_opt_out_shapes() {
        let yes1 = serde_json::json!({ "fields": { "ai": "exclude" } });
        let yes2 = serde_json::json!({ "fields": { "ai": "Excluded" } });
        let yes3 = serde_json::json!({ "fields": { "ai_exclude": "true" } });
        let no1 = serde_json::json!({ "fields": { "ai": "summarize" } });
        let no2 = serde_json::json!({ "fields": {} });
        let no3 = serde_json::json!({});
        assert!(frontmatter_excludes(&yes1));
        assert!(frontmatter_excludes(&yes2));
        assert!(frontmatter_excludes(&yes3));
        assert!(!frontmatter_excludes(&no1));
        assert!(!frontmatter_excludes(&no2));
        assert!(!frontmatter_excludes(&no3));
    }
}
