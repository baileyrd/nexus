//! Guard: `nexus-plugin-api` must never depend on kernel-internal crates.
//!
//! Why: `nexus-plugin-api` is the stable contract community plugins compile
//! against. If a kernel-internal crate (`nexus-kernel`, `nexus-plugins`) leaks
//! into its dep graph, every kernel refactor silently becomes a plugin-ABI
//! break — exactly what the F-2.1.1 extraction was meant to prevent.
//!
//! See `docs/PRDs/backlog/` (F-2.1.1 close, 2026-04-22) for context.

use std::collections::BTreeSet;
use std::path::PathBuf;

// Note: `nexus-app` is listed historically — the crate was deleted under
// Phase 4 WI-37 (2026-04-24). Keeping the literal in the allowlist is cheap
// and guards against the name being reused by an unrelated crate.
const FORBIDDEN: &[&str] = &[
    "nexus-kernel",
    "nexus-plugins",
    "nexus-app",
    "nexus-bootstrap",
    "nexus-cli",
    "nexus-tui",
    "nexus-storage",
    "nexus-security",
    "nexus-kv",
    "nexus-database",
    "nexus-ai",
    "nexus-mcp",
    "nexus-git",
    "nexus-editor",
    "nexus-terminal",
    "nexus-agent",
    "nexus-skills",
    "nexus-workflow",
    "nexus-formats",
    "nexus-theme",
];

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

fn collect_dep_names(table: Option<&toml::Value>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(toml::Value::Table(t)) = table {
        for k in t.keys() {
            out.insert(k.clone());
        }
    }
    out
}

#[test]
fn cargo_toml_has_no_kernel_internal_dependencies() {
    let path = manifest_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let parsed: toml::Value = toml::from_str(&raw)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));

    let mut all_deps: BTreeSet<String> = BTreeSet::new();
    all_deps.extend(collect_dep_names(parsed.get("dependencies")));
    all_deps.extend(collect_dep_names(parsed.get("build-dependencies")));
    if let Some(toml::Value::Table(targets)) = parsed.get("target") {
        for (_, cfg) in targets {
            all_deps.extend(collect_dep_names(cfg.get("dependencies")));
            all_deps.extend(collect_dep_names(cfg.get("build-dependencies")));
        }
    }

    let violations: Vec<&str> = FORBIDDEN
        .iter()
        .copied()
        .filter(|name| all_deps.contains(*name))
        .collect();

    assert!(
        violations.is_empty(),
        "nexus-plugin-api must stay kernel-free, but its Cargo.toml lists \
         forbidden kernel-internal dependencies: {violations:?}.\n\n\
         This crate is the stable contract community plugins compile against. \
         Adding a kernel-internal dep means every kernel refactor silently \
         becomes a plugin-ABI break — see docs/PRDs/backlog/ \
         (F-2.1.1) for the full rationale.\n\n\
         If you genuinely need a type from one of these crates, move the type \
         into nexus-plugin-api (or a new shared crate) instead of pulling the \
         kernel into the plugin contract."
    );
}

#[test]
fn source_files_do_not_reference_kernel_internal_crates() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders: Vec<(PathBuf, String, &str)> = Vec::new();

    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read_dir src").flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }

    let mut files = Vec::new();
    walk(&src, &mut files);

    for file in files {
        let contents = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", file.display()));
        for forbidden in FORBIDDEN {
            let snake = forbidden.replace('-', "_");
            for line in contents.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || trimmed.starts_with("//!") {
                    continue;
                }
                if line.contains(&snake) {
                    offenders.push((file.clone(), line.to_string(), forbidden));
                    break;
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "nexus-plugin-api source must not reference kernel-internal crates, \
         but found:\n{}",
        offenders
            .iter()
            .map(|(f, line, name)| format!(
                "  {} -> mentions `{}`: {}",
                f.display(),
                name,
                line.trim()
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
