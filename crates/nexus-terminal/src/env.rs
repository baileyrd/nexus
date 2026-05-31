//! Environment-variable resolution + `.env` parsing (PRD-09 §8).
//!
//! # What this module does
//!
//! - **Parses `.env` files** into an ordered `Vec<(key, value)>` so
//!   callers can feed them into [`crate::SessionConfig::env`] without
//!   losing declaration order (shell `export` ordering, display order
//!   in `nexus term env --show`, etc.).
//! - **Layers four env sources** per the PRD precedence list:
//!   command-level (highest) → `.env` file → shell-inherited →
//!   Nexus-injected (lowest). Deduplicates on key, later layers shadow
//!   earlier layers.
//! - **Interpolates `${VAR}` / `$VAR`** across the merged set, iterating
//!   up to 10 times so chained references (`A=1; B=${A}; C=${B}`) all
//!   land. Missing variables stay literal.
//! - **Detects secret-looking keys** (`*API*`, `*KEY*`, `*SECRET*`,
//!   `*TOKEN*`, `*PASSWORD*`, case-insensitive) so UI surfaces can mask
//!   values in logs and the process panel.
//!
//! # What this module does NOT do
//!
//! - Spawn shells. The returned `Vec<(K, V)>` is ready to pass to
//!   [`crate::SessionConfig::env`]; orchestration belongs to the caller.
//! - Source the user's shell profile (§1.3 "source ~/.bashrc"). That
//!   lives on the `Session`-spawn path and is orthogonal to this
//!   pure-function resolver.
//! - Write `.env` files. Parsing is one-way.

use std::collections::HashSet;
use std::path::Path;

/// Parse a `.env` file at `path` into an ordered `Vec<(key, value)>`.
///
/// Format (PRD-09 §8.2):
///
/// - Lines starting with `#` (after leading whitespace) are comments.
/// - Blank lines are skipped.
/// - `KEY=VALUE` pairs are collected. Keys are trimmed; values preserve
///   internal whitespace after the first `=`.
/// - If the value is wrapped in matching `"..."` or `'...'` the surrounding
///   quotes are stripped. Un-balanced quotes are preserved literally.
/// - Inline `# comment` tails are **not** stripped — a value may legally
///   contain `#` because unquoted comments-in-values is a common footgun
///   and handling it reliably requires a real parser. Use quotes if the
///   literal `#` matters.
/// - Duplicate keys: the later declaration wins; both are returned in
///   order so callers inspecting the raw parse can see conflicts.
///
/// # Errors
/// Returns the underlying I/O error if the file cannot be read.
pub fn parse_env_file(path: &Path) -> std::io::Result<Vec<(String, String)>> {
    let contents = std::fs::read_to_string(path)?;
    Ok(parse_env_text(&contents))
}

/// Parse `.env`-formatted text directly — pulled out of [`parse_env_file`]
/// so tests can feed inline fixtures without touching the filesystem.
#[must_use]
pub fn parse_env_text(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(eq) = trimmed.find('=') else {
            continue;
        };
        let key = trimmed[..eq].trim();
        if key.is_empty() {
            continue;
        }
        let raw_value = trimmed[eq + 1..].trim();

        let value = if raw_value.len() >= 2 {
            let first = raw_value.as_bytes()[0];
            let last = raw_value.as_bytes()[raw_value.len() - 1];
            if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
                raw_value[1..raw_value.len() - 1].to_string()
            } else {
                raw_value.to_string()
            }
        } else {
            raw_value.to_string()
        };

        out.push((key.to_string(), value));
    }
    out
}

/// Merge four ordered env layers into a single map, honouring the PRD §8.1
/// precedence (highest priority last in the parameter list would be
/// idiomatic for a `merge`, but PRD ordering reads top-down, so this
/// function takes the layers in PRD order — `command_env` first).
///
/// Resolution: command → `.env` → shell → injected. Each key keeps the
/// value from the highest-priority layer that declared it.
///
/// The returned `Vec` is ordered by first-sighting, so downstream callers
/// (e.g. the process panel's env inspector) can render a stable, readable
/// list. Duplicate keys across layers are collapsed; the winning layer's
/// value replaces the placeholder at the first-sighting position.
///
/// # Panics
/// Never. The internal `map.remove(&k).expect(...)` is unreachable by
/// construction — `k` is only added to `order` when it was simultaneously
/// inserted into `map`, and each key is removed at most once because the
/// iteration walks `order` which contains unique keys.
#[must_use]
pub fn resolve_env(
    command_env: &[(String, String)],
    env_file: &[(String, String)],
    shell_env: &[(String, String)],
    nexus_injected: &[(String, String)],
) -> Vec<(String, String)> {
    // Walk layers from lowest precedence to highest so later writes win.
    // Preserve first-sighting order with a separate `order` vector.
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    let layers = [nexus_injected, shell_env, env_file, command_env];
    for layer in layers {
        for (k, v) in layer {
            if seen.insert(k.clone()) {
                order.push(k.clone());
            }
            map.insert(k.clone(), v.clone());
        }
    }

    order
        .into_iter()
        .map(|k| {
            let v = map
                .remove(&k)
                .expect("order key always present in map (just inserted above)");
            (k, v)
        })
        .collect()
}

/// Maximum interpolation iterations before we give up and leave
/// whatever references remain literal. PRD §8.3 calls for 10.
const MAX_INTERPOLATION_PASSES: usize = 10;

/// Expand `${VAR}` and `$VAR` references inside every value, using the
/// same set as the lookup table. Converges in at most
/// [`MAX_INTERPOLATION_PASSES`] passes; unresolved references (typo, or
/// genuinely missing var) are left as-is in the output.
///
/// Input is the fully-resolved env from [`resolve_env`]; order is
/// preserved.
#[must_use]
pub fn interpolate_env(env: &[(String, String)]) -> Vec<(String, String)> {
    let mut current: Vec<(String, String)> = env.to_vec();
    for _ in 0..MAX_INTERPOLATION_PASSES {
        let lookup: std::collections::HashMap<&str, &str> = current
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let mut changed = false;
        let next: Vec<(String, String)> = current
            .iter()
            .map(|(k, v)| {
                let expanded = expand_refs(v, &lookup);
                if expanded != *v {
                    changed = true;
                }
                (k.clone(), expanded)
            })
            .collect();
        current = next;
        if !changed {
            break;
        }
    }
    current
}

/// One-pass expansion of `$NAME` / `${NAME}` references in `value`.
/// Unknown names are preserved as-is so a typo is visible in the output.
fn expand_refs(value: &str, lookup: &std::collections::HashMap<&str, &str>) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }

        // Peek at the character after '$'.
        if i + 1 >= bytes.len() {
            out.push('$');
            i += 1;
            continue;
        }

        if bytes[i + 1] == b'{' {
            // ${NAME} form — find matching `}`.
            if let Some(close) = value[i + 2..].find('}') {
                let name = &value[i + 2..i + 2 + close];
                if let Some(v) = lookup.get(name) {
                    out.push_str(v);
                } else {
                    out.push_str(&value[i..=i + 2 + close]);
                }
                i += 2 + close + 1;
                continue;
            }
            // No closing brace — emit verbatim and move on.
            out.push('$');
            i += 1;
            continue;
        }

        // $NAME bare form — `NAME` is ascii alnum + underscore,
        // starting with alpha or underscore.
        let first = bytes[i + 1];
        let is_name_start = first.is_ascii_alphabetic() || first == b'_';
        if !is_name_start {
            out.push('$');
            i += 1;
            continue;
        }
        let mut end = i + 2;
        while end < bytes.len() {
            let c = bytes[end];
            if c.is_ascii_alphanumeric() || c == b'_' {
                end += 1;
            } else {
                break;
            }
        }
        let name = &value[i + 1..end];
        if let Some(v) = lookup.get(name) {
            out.push_str(v);
        } else {
            out.push_str(&value[i..end]);
        }
        i = end;
    }
    out
}

/// Is this env key almost certainly a secret? Case-insensitive substring
/// match against the PRD §8.3 wildcard list (`*API*`, `*KEY*`, `*SECRET*`,
/// `*TOKEN*`, `*PASSWORD*`).
#[must_use]
pub fn is_secret_key(key: &str) -> bool {
    const NEEDLES: &[&str] = &["API", "KEY", "SECRET", "TOKEN", "PASSWORD"];
    let upper = key.to_ascii_uppercase();
    NEEDLES.iter().any(|n| upper.contains(n))
}

/// Replacement shown in place of secret values by [`mask_secrets`].
pub const REDACTED: &str = "[REDACTED]";

/// Produce a display-only view of the env where every secret-keyed value
/// is replaced by [`REDACTED`]. Used by the process panel and by logging
/// so tokens don't leak into text surfaces.
#[must_use]
pub fn mask_secrets(env: &[(String, String)]) -> Vec<(String, String)> {
    env.iter()
        .map(|(k, v)| {
            if is_secret_key(k) {
                (k.clone(), REDACTED.to_string())
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_env_text ─────────────────────────────────────────────────

    #[test]
    fn parse_basic_key_value_pairs() {
        let env = parse_env_text("FOO=bar\nBAZ=qux");
        assert_eq!(
            env,
            vec![("FOO".into(), "bar".into()), ("BAZ".into(), "qux".into()),]
        );
    }

    #[test]
    fn parse_strips_balanced_double_quotes() {
        let env = parse_env_text(r#"MSG="hello world""#);
        assert_eq!(env, vec![("MSG".into(), "hello world".into())]);
    }

    #[test]
    fn parse_strips_balanced_single_quotes() {
        let env = parse_env_text("MSG='hi there'");
        assert_eq!(env, vec![("MSG".into(), "hi there".into())]);
    }

    #[test]
    fn parse_preserves_unbalanced_quotes() {
        // A leading quote with no closer is a literal quote, not an
        // opening delimiter.
        let env = parse_env_text(r#"MSG="mismatched"#);
        assert_eq!(env, vec![("MSG".into(), r#""mismatched"#.into())]);
    }

    #[test]
    fn parse_skips_comments_and_blank_lines() {
        let env = parse_env_text("# top comment\n\n  # indented comment\n\nFOO=bar\n\n");
        assert_eq!(env, vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn parse_skips_malformed_lines_without_equals() {
        let env = parse_env_text("not_a_pair\nFOO=bar\nalso_bad");
        assert_eq!(env, vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn parse_preserves_internal_whitespace_in_value() {
        let env = parse_env_text("K=  multi  space  ");
        // Leading/trailing whitespace around the whole value is trimmed
        // (that's standard .env behaviour) but the internal spaces stay.
        assert_eq!(env, vec![("K".into(), "multi  space".into())]);
    }

    #[test]
    fn parse_preserves_order_and_duplicates() {
        let env = parse_env_text("A=1\nB=2\nA=3");
        assert_eq!(
            env,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "2".into()),
                ("A".into(), "3".into()),
            ]
        );
    }

    #[test]
    fn parse_env_file_reads_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "FROM_DISK=yes\n").unwrap();
        let env = parse_env_file(&path).unwrap();
        assert_eq!(env, vec![("FROM_DISK".into(), "yes".into())]);
    }

    // ── resolve_env ────────────────────────────────────────────────────

    #[test]
    fn resolve_prefers_command_over_env_file_over_shell_over_injected() {
        let command = vec![("LEVEL".into(), "command".into())];
        let env_file = vec![("LEVEL".into(), "env_file".into())];
        let shell = vec![("LEVEL".into(), "shell".into())];
        let injected = vec![("LEVEL".into(), "injected".into())];
        let out = resolve_env(&command, &env_file, &shell, &injected);
        assert_eq!(out, vec![("LEVEL".into(), "command".into())]);
    }

    #[test]
    fn resolve_merges_disjoint_layers() {
        let command = vec![("CMD".into(), "c".into())];
        let env_file = vec![("FILE".into(), "f".into())];
        let shell = vec![("SHELL_VAR".into(), "s".into())];
        let injected = vec![("INJ".into(), "i".into())];
        let out = resolve_env(&command, &env_file, &shell, &injected);
        // Order: injected first (lowest), then shell, env_file, command
        // — first-sighting preserved.
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].0, "INJ");
        assert_eq!(out[1].0, "SHELL_VAR");
        assert_eq!(out[2].0, "FILE");
        assert_eq!(out[3].0, "CMD");
    }

    #[test]
    fn resolve_later_layer_overrides_earlier_without_moving_position() {
        // Shell declares FOO=shell first; command later overrides.
        // Expected: FOO stays at its first-sighting position (where
        // shell put it) but with the command's value.
        let command = vec![("FOO".into(), "command".into())];
        let shell = vec![("FOO".into(), "shell".into())];
        let out = resolve_env(&command, &[], &shell, &[]);
        assert_eq!(out, vec![("FOO".into(), "command".into())]);
    }

    // ── interpolate_env ───────────────────────────────────────────────

    #[test]
    fn interpolate_resolves_simple_brace_ref() {
        let env = vec![
            ("HOME".into(), "/home/me".into()),
            ("PATH".into(), "${HOME}/bin".into()),
        ];
        let out = interpolate_env(&env);
        assert_eq!(out[1].1, "/home/me/bin");
    }

    #[test]
    fn interpolate_resolves_simple_bare_ref() {
        let env = vec![
            ("HOME".into(), "/home/me".into()),
            ("CONFIG".into(), "$HOME/.config".into()),
        ];
        let out = interpolate_env(&env);
        assert_eq!(out[1].1, "/home/me/.config");
    }

    #[test]
    fn interpolate_resolves_chained_refs_up_to_cap() {
        let env = vec![
            ("A".into(), "1".into()),
            ("B".into(), "${A}".into()),
            ("C".into(), "${B}".into()),
        ];
        let out = interpolate_env(&env);
        assert_eq!(out[1].1, "1");
        assert_eq!(out[2].1, "1");
    }

    #[test]
    fn interpolate_leaves_unknown_refs_literal() {
        let env = vec![("X".into(), "${MISSING}/path".into())];
        let out = interpolate_env(&env);
        assert_eq!(out[0].1, "${MISSING}/path");
    }

    #[test]
    fn interpolate_leaves_lone_dollar_literal() {
        let env = vec![("A".into(), "cost: $5 total".into())];
        let out = interpolate_env(&env);
        assert_eq!(out[0].1, "cost: $5 total");
    }

    #[test]
    fn interpolate_handles_mixed_refs_in_one_value() {
        let env = vec![
            ("USER".into(), "alice".into()),
            ("HOST".into(), "box".into()),
            ("LABEL".into(), "$USER@${HOST}".into()),
        ];
        let out = interpolate_env(&env);
        assert_eq!(out[2].1, "alice@box");
    }

    #[test]
    fn interpolate_survives_cycles_without_panicking() {
        // A refers to B, B refers to A — after the iteration cap we
        // should simply stop expanding. Whatever we land on is fine as
        // long as the function terminates.
        let env = vec![("A".into(), "${B}".into()), ("B".into(), "${A}".into())];
        let _ = interpolate_env(&env);
    }

    // ── secret masking ────────────────────────────────────────────────

    #[test]
    fn is_secret_matches_case_insensitive_substring() {
        assert!(is_secret_key("API_KEY"));
        assert!(is_secret_key("openai_api_key"));
        assert!(is_secret_key("GITHUB_TOKEN"));
        assert!(is_secret_key("DB_PASSWORD"));
        assert!(is_secret_key("session_secret"));
        assert!(is_secret_key("private_key_path"));
    }

    #[test]
    fn is_secret_rejects_non_secret_keys() {
        assert!(!is_secret_key("PATH"));
        assert!(!is_secret_key("HOME"));
        assert!(!is_secret_key("DEBUG"));
        assert!(!is_secret_key("USER"));
    }

    #[test]
    fn mask_secrets_replaces_only_secret_values() {
        let env = vec![
            ("PATH".into(), "/usr/bin".into()),
            ("API_KEY".into(), "sk-super-secret".into()),
            ("DEBUG".into(), "1".into()),
        ];
        let masked = mask_secrets(&env);
        assert_eq!(masked[0], ("PATH".into(), "/usr/bin".into()));
        assert_eq!(masked[1], ("API_KEY".into(), REDACTED.into()));
        assert_eq!(masked[2], ("DEBUG".into(), "1".into()));
    }
}
