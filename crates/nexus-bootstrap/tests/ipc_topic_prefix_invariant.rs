//! BL-137 — static source-level check that every `.publish(...)` call in
//! an in-tree core plugin (or `nexus-bootstrap` helper) emits a topic that
//! lies in its own plugin namespace, **or** is one of the kernel-owned
//! shared topics in `nexus_kernel::event_bus::is_kernel_owned_shared_topic`.
//!
//! ## Why a static scan and not a runtime test
//!
//! The kernel already enforces this property at runtime
//! (`PluginContext::publish` returns `BusError::TypeIdNamespaceMismatch`
//! for a foreign topic — see `nexus-bootstrap/tests/event_bus_anti_spoofing.rs`).
//! But many publish sites are wrapped in `let _ = ctx.publish(...)` because
//! the call is best-effort, so a namespace mismatch turns into a silent
//! drop at runtime. A new handler that posts to the wrong topic would
//! compile, pass tests, and quietly never fire.
//!
//! This static scan catches the mistake at test time. It is intentionally
//! conservative: it only inspects publish calls whose first argument is a
//! string literal or one of a small set of well-known constants. Calls
//! whose topic is computed (`&topic`, `format!(...)`) are reported in a
//! diagnostic block but not failed — those are by design for fan-out
//! emitters (e.g. the AI runtime forwards a parameter-derived `AiEvent`
//! topic).
//!
//! ## When this test fails
//!
//! Either:
//!
//! - You moved a publish site to the wrong namespace by accident. Fix the
//!   topic literal.
//! - You added a legitimate kernel-shared topic (cross-plugin fan-out
//!   channel, like the activity timeline). Extend
//!   `KERNEL_OWNED_SHARED_TOPICS` in `nexus-kernel/src/event_bus.rs`
//!   first, then this test will accept it.
//! - You added a new core plugin whose crate isn't in `OWNERS` below.
//!   Add the `(crate-name, plugin-id)` row.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// `(crate name, owner plugin id)`. The owner plugin id is the value of
/// `PLUGIN_ID` declared in the crate's `core_plugin.rs`. A publish call
/// found anywhere under `crates/<crate>/src/**/*.rs` is expected to emit
/// a topic in this id's namespace.
const OWNERS: &[(&str, &str)] = &[
    ("nexus-agent", "com.nexus.agent"),
    ("nexus-ai", "com.nexus.ai"),
    ("nexus-ai-runtime", "com.nexus.ai.runtime"),
    ("nexus-audio", "com.nexus.audio"),
    ("nexus-comments", "com.nexus.comments"),
    ("nexus-database", "com.nexus.database"),
    ("nexus-editor", "com.nexus.editor"),
    ("nexus-formats", "com.nexus.formats"),
    ("nexus-git", "com.nexus.git"),
    ("nexus-linkpreview", "com.nexus.linkpreview"),
    ("nexus-lsp", "com.nexus.lsp"),
    ("nexus-mcp", "com.nexus.mcp.host"),
    ("nexus-notifications", "com.nexus.notifications"),
    ("nexus-security", "com.nexus.security"),
    ("nexus-skills", "com.nexus.skills"),
    ("nexus-storage", "com.nexus.storage"),
    ("nexus-templates", "com.nexus.templates"),
    ("nexus-terminal", "com.nexus.terminal"),
    ("nexus-theme", "com.nexus.theme"),
    ("nexus-workflow", "com.nexus.workflow"),
];

/// Publish sites in `nexus-bootstrap/src/**/*.rs` run under the invoker's
/// `PluginContext` (CLI / TUI / shell), not under a fixed plugin id.
/// We allow these to publish to the `dream_cycle` namespace because that
/// namespace has no owning core plugin — it's the bootstrap-side dream
/// cycle scheduler. If you add a new bootstrap-side publish, the topic
/// must either be in this set or be a kernel-shared topic.
const BOOTSTRAP_ALLOWED_PREFIXES: &[&str] = &["com.nexus.dream_cycle."];

/// Kernel-owned shared topics that any plugin may publish to. Must match
/// `nexus-kernel::event_bus::KERNEL_OWNED_SHARED_TOPICS` exactly — the
/// kernel is the source of truth; this is a mirror because the const is
/// private to the bus module.
const KERNEL_SHARED_TOPICS: &[&str] = &[
    // nexus-types::activity::ACTIVITY_APPENDED_TOPIC
    "com.nexus.activity.appended",
];

#[test]
fn every_publish_call_emits_in_namespace() {
    let workspace = workspace_root();
    let crates_dir = workspace.join("crates");

    let mut violations: Vec<String> = Vec::new();
    let mut dynamic: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (crate_name, owner_id) in OWNERS {
        let crate_root = crates_dir.join(crate_name).join("src");
        if !crate_root.exists() {
            panic!(
                "OWNERS row references {crate_name} but {} does not exist — \
                 either the crate was removed or OWNERS is stale",
                crate_root.display()
            );
        }
        for path in walk_rust_files(&crate_root) {
            let text = fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!("read {}: {}", path.display(), e);
            });
            scan_publishes(
                &text,
                &path,
                |topic_literal, line_no| {
                    if !topic_in_namespace(topic_literal, owner_id)
                        && !KERNEL_SHARED_TOPICS.contains(&topic_literal)
                    {
                        violations.push(format!(
                            "  {}:{}: publish topic {:?} is outside namespace {:?} \
                             and is not a kernel-shared topic",
                            path.display(),
                            line_no,
                            topic_literal,
                            owner_id,
                        ));
                    }
                },
                |non_literal, line_no| {
                    dynamic
                        .entry(format!("{} ({})", path.display(), owner_id))
                        .or_default()
                        .push(format!("L{line_no}: {non_literal}"));
                },
            );
        }
    }

    // nexus-bootstrap/src is a special case — see `BOOTSTRAP_ALLOWED_PREFIXES`.
    let bootstrap_src = workspace.join("crates/nexus-bootstrap/src");
    for path in walk_rust_files(&bootstrap_src) {
        let text = fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("read {}: {}", path.display(), e);
        });
        scan_publishes(
            &text,
            &path,
            |topic_literal, line_no| {
                let in_allowed_prefix = BOOTSTRAP_ALLOWED_PREFIXES
                    .iter()
                    .any(|p| topic_literal.starts_with(p));
                if !in_allowed_prefix && !KERNEL_SHARED_TOPICS.contains(&topic_literal) {
                    violations.push(format!(
                        "  {}:{}: bootstrap publish topic {:?} is neither in \
                         BOOTSTRAP_ALLOWED_PREFIXES nor a kernel-shared topic",
                        path.display(),
                        line_no,
                        topic_literal,
                    ));
                }
            },
            |non_literal, line_no| {
                dynamic
                    .entry(format!("{} (bootstrap)", path.display()))
                    .or_default()
                    .push(format!("L{line_no}: {non_literal}"));
            },
        );
    }

    if !violations.is_empty() {
        let dynamic_report = if dynamic.is_empty() {
            String::new()
        } else {
            let mut s = String::from(
                "\n\nDynamic publish sites (skipped by the static check — visual review only):\n",
            );
            for (k, v) in &dynamic {
                s.push_str(&format!("  {k}\n"));
                for line in v {
                    s.push_str(&format!("    {line}\n"));
                }
            }
            s
        };
        panic!(
            "IPC topic-prefix invariant violated — a plugin is publishing \
             to a topic outside its own namespace, and the topic is not in \
             the kernel-shared allowlist:\n{}{}",
            violations.join("\n"),
            dynamic_report,
        );
    }
}

/// Smoke-test: confirm the OWNERS table matches the `PLUGIN_ID` constant
/// declared in each crate's `core_plugin.rs`. Catches typos and forgotten
/// renames; OWNERS being wrong silently would silently weaken the check
/// above.
#[test]
fn owners_table_matches_plugin_id_constants() {
    let crates_dir = workspace_root().join("crates");
    // Crates that have centralized `PLUGIN_ID` to `nexus_types::plugin_ids`
    // no longer carry the literal in their own src tree — accept the
    // canonical registry as proof instead.
    let plugin_ids_registry = crates_dir
        .join("nexus-types")
        .join("src")
        .join("plugin_ids.rs");
    let mut errors = Vec::new();

    for (crate_name, expected_id) in OWNERS {
        let core = crates_dir
            .join(crate_name)
            .join("src")
            .join("core_plugin.rs");
        let src = crates_dir.join(crate_name).join("src");
        // PLUGIN_ID lives in `core_plugin.rs` for most crates, but
        // a couple declare it in `lib.rs` instead (e.g. nexus-ai-runtime).
        // Walk the tree rather than hard-requiring a fixed file path.
        if !core.exists() || !file_contains(&core, expected_id) {
            if !found_plugin_id_in_tree(&src, expected_id)
                && !file_contains(&plugin_ids_registry, expected_id)
            {
                errors.push(format!(
                    "{crate_name}: expected literal {expected_id:?} \
                     somewhere under src/ or in nexus-types/src/plugin_ids.rs \
                     — OWNERS may be stale"
                ));
            }
        }
    }

    assert!(
        errors.is_empty(),
        "OWNERS table is stale:\n{}",
        errors.join("\n")
    );
}

fn file_contains(path: &Path, needle: &str) -> bool {
    fs::read_to_string(path)
        .map(|t| t.contains(&format!("\"{needle}\"")))
        .unwrap_or(false)
}

fn found_plugin_id_in_tree(root: &Path, expected_id: &str) -> bool {
    for path in walk_rust_files(root) {
        if let Ok(text) = fs::read_to_string(&path) {
            if text.contains(&format!("\"{expected_id}\"")) {
                return true;
            }
        }
    }
    false
}

fn topic_in_namespace(topic: &str, plugin_id: &str) -> bool {
    if topic == plugin_id {
        return true;
    }
    topic
        .strip_prefix(plugin_id)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// Iterate `.publish(` call sites in `source`. Calls `on_literal` for
/// topics whose first argument is a `"..."` literal, and `on_dynamic`
/// for any other shape (variable, format!, constant reference). Line
/// numbers are 1-based.
fn scan_publishes<L, D>(source: &str, _path: &Path, mut on_literal: L, mut on_dynamic: D)
where
    L: FnMut(&str, usize),
    D: FnMut(&str, usize),
{
    // Two-state scan: find `.publish(` then peek at the next non-whitespace.
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut line_no = 1;
    let needle = b".publish(";
    while i + needle.len() < bytes.len() {
        if bytes[i] == b'\n' {
            line_no += 1;
        }
        if &bytes[i..i + needle.len()] == needle {
            // Skip whitespace after the open paren.
            let mut j = i + needle.len();
            let mut local_line = line_no;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\n') {
                if bytes[j] == b'\n' {
                    local_line += 1;
                }
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'"' {
                // String literal — read until closing unescaped quote.
                let start = j + 1;
                let mut k = start;
                while k < bytes.len() {
                    if bytes[k] == b'\\' {
                        k += 2;
                        continue;
                    }
                    if bytes[k] == b'"' {
                        break;
                    }
                    k += 1;
                }
                if k < bytes.len() {
                    let literal = &source[start..k];
                    on_literal(literal, local_line);
                }
            } else if j < bytes.len() {
                // Non-literal — capture the token up to comma or close
                // paren for the diagnostic.
                let mut k = j;
                let mut depth = 1;
                while k < bytes.len() && depth > 0 {
                    match bytes[k] {
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        b',' if depth == 1 => break,
                        _ => {}
                    }
                    k += 1;
                }
                let snippet = source[j..k.min(source.len())].trim().replace('\n', " ");
                if !snippet.is_empty() {
                    on_dynamic(&snippet, local_line);
                }
            }
            i = j;
            line_no = local_line;
            continue;
        }
        i += 1;
    }
}

fn walk_rust_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            if let Ok(text) = fs::read_to_string(&candidate) {
                if text.contains("[workspace]") {
                    return dir;
                }
            }
        }
        if !dir.pop() {
            panic!(
                "failed to locate workspace root from {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

#[cfg(test)]
mod self_test {
    use super::*;

    #[test]
    fn topic_namespace_helper_matches_kernel_semantics() {
        assert!(topic_in_namespace("com.nexus.foo", "com.nexus.foo"));
        assert!(topic_in_namespace("com.nexus.foo.bar", "com.nexus.foo"));
        assert!(!topic_in_namespace("com.nexus.foobar", "com.nexus.foo"));
        assert!(!topic_in_namespace("com.nexus.foo-evil.x", "com.nexus.foo"));
    }

    #[test]
    fn scanner_finds_literal_topic() {
        let src = r#"
            fn f() {
                let _ = ctx.publish("com.example.x", payload);
            }
        "#;
        let mut hits = Vec::new();
        let mut dynamics = Vec::new();
        scan_publishes(
            src,
            Path::new("test.rs"),
            |t, l| hits.push((t.to_string(), l)),
            |d, l| dynamics.push((d.to_string(), l)),
        );
        assert_eq!(hits, vec![("com.example.x".to_string(), 3)]);
        assert!(dynamics.is_empty());
    }

    #[test]
    fn scanner_reports_dynamic_topic() {
        let src = r#"
            fn f() {
                let _ = ctx.publish(&topic, payload);
            }
        "#;
        let mut hits = Vec::new();
        let mut dynamics = Vec::new();
        scan_publishes(
            src,
            Path::new("test.rs"),
            |t, l| hits.push((t.to_string(), l)),
            |d, l| dynamics.push((d.to_string(), l)),
        );
        assert!(hits.is_empty());
        assert_eq!(dynamics.len(), 1);
        assert!(dynamics[0].0.starts_with('&'));
    }
}
