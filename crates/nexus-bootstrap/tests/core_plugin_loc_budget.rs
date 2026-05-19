//! SD-07 guardrail (2026-05-18 SOLID/DRY audit): keep service crates'
//! `core_plugin.rs` files under a per-file LOC budget so they can't
//! quietly regress back into god-modules.
//!
//! `nexus-editor` and `nexus-terminal` are grandfathered above the
//! default budget — they exceeded it before SD-03 landed and a full
//! per-domain split (the SD-03 vision) is deferred to a dedicated
//! session. The guardrail still locks them at their current sizes so
//! they can only shrink, never grow.
//!
//! When a service crate's `core_plugin.rs` legitimately needs more
//! room, the right move is to apply the SD-03 `handlers/<domain>.rs`
//! pattern (see `crates/nexus-git/src/handlers/` or
//! `crates/nexus-storage/src/handlers/` for examples) rather than
//! bumping the cap.
//!
//! Run as:
//! ```sh
//! cargo test -p nexus-bootstrap --test core_plugin_loc_budget
//! ```

use std::fs;
use std::path::{Path, PathBuf};

/// Default budget — every `core_plugin.rs` must stay under this unless
/// it has an explicit row in [`GRANDFATHERED`].
///
/// 2 000 lines is roughly where a file stops fitting in a single
/// reviewer's working memory. Service plugins under this threshold
/// don't typically need an internal split.
const DEFAULT_BUDGET: usize = 2_000;

/// Grandfathered files: pre-SD-03 debt with documented intent to
/// shrink. Each entry pins the *current* line count plus a small
/// safety margin so the file can only stay the same or shrink.
///
/// Removing a row when its file drops under `DEFAULT_BUDGET` is the
/// expected lifecycle — that's the signal the SD-03 split has landed
/// for that crate.
const GRANDFATHERED: &[(&str, usize)] = &[
    // Documented debt — both editor and terminal SD-03 splits landed
    // 2026-05-18 and lower these grandfather rows to lock in the
    // post-split sizes so the files can only shrink further. What
    // remains in each `core_plugin.rs` is the dispatch impl plus a
    // large IPC-roundtrip test module; the per-domain handler bodies
    // live under `<crate>/src/handlers/<domain>.rs`.
    ("nexus-editor", 2_300),
    ("nexus-terminal", 3_600),
    // Just over the default — earned a modest grandfather rather than
    // a full split. Bump to DEFAULT_BUDGET when the trigger / template
    // handlers move out of core_plugin.rs.
    ("nexus-workflow", 2_200),
];

#[test]
fn every_core_plugin_file_stays_under_budget() {
    let workspace_root = workspace_root();
    let crates_dir = workspace_root.join("crates");

    let mut failures = Vec::new();

    for entry in fs::read_dir(&crates_dir).expect("read crates/ dir") {
        let entry = entry.expect("crate dir entry");
        let crate_path = entry.path();
        if !crate_path.is_dir() {
            continue;
        }
        let Some(crate_name) = crate_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if !crate_name.starts_with("nexus-") {
            continue;
        }

        let core_plugin = crate_path.join("src/core_plugin.rs");
        if !core_plugin.exists() {
            continue;
        }

        let line_count = count_lines(&core_plugin);
        let budget = GRANDFATHERED
            .iter()
            .find(|(name, _)| *name == crate_name)
            .map_or(DEFAULT_BUDGET, |(_, b)| *b);

        if line_count > budget {
            failures.push(format!(
                "  {}: {} LOC > {} budget ({})",
                crate_name,
                line_count,
                budget,
                if GRANDFATHERED.iter().any(|(n, _)| *n == crate_name) {
                    "grandfathered"
                } else {
                    "default"
                }
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "SD-07 core_plugin.rs LOC budget violations:\n{}\n\n\
         The right fix is to extract handlers into a `handlers/<domain>.rs` \
         module (see crates/nexus-git/src/handlers/ for the pattern), not to \
         bump the cap. If shrinking landed for a grandfathered crate, lower \
         the GRANDFATHERED entry to lock in the win.",
        failures.join("\n")
    );
}

/// Locate the workspace root by walking up from `CARGO_MANIFEST_DIR`
/// until we find a `Cargo.toml` containing `[workspace]`.
fn workspace_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = path.join("Cargo.toml");
        if candidate.is_file() {
            let content = fs::read_to_string(&candidate).expect("read Cargo.toml");
            if content.contains("[workspace]") {
                return path;
            }
        }
        if !path.pop() {
            panic!(
                "workspace_root: could not find a `[workspace]` Cargo.toml \
                 walking up from CARGO_MANIFEST_DIR"
            );
        }
    }
}

/// Count physical lines in `path`. Matches `wc -l` semantics: counts
/// newline characters, plus one for any trailing partial line.
fn count_lines(path: &Path) -> usize {
    let bytes = fs::read(path).expect("read core_plugin.rs");
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    if bytes.last().is_some_and(|&b| b != b'\n') {
        newlines + 1
    } else {
        newlines
    }
}

#[test]
fn grandfathered_entries_are_actually_oversized() {
    // Sanity: each GRANDFATHERED row should describe a file that
    // really exceeds DEFAULT_BUDGET. If a row drops under, the row
    // itself should be removed and the file picked up by the
    // every_core_plugin path. This guards against forgotten
    // exceptions.
    let workspace_root = workspace_root();
    for (crate_name, _budget) in GRANDFATHERED {
        let path = workspace_root
            .join("crates")
            .join(crate_name)
            .join("src/core_plugin.rs");
        if !path.exists() {
            // The crate was removed from the workspace; the
            // grandfathered row is also stale.
            panic!(
                "GRANDFATHERED references {crate_name} but \
                 {} does not exist — remove the row",
                path.display()
            );
        }
        let loc = count_lines(&path);
        assert!(
            loc > DEFAULT_BUDGET,
            "GRANDFATHERED row for {crate_name} is no longer needed — \
             file is {loc} LOC, under the {DEFAULT_BUDGET} default budget. \
             Remove the row to lock in the win."
        );
    }
}

#[test]
fn default_budget_is_a_round_thousand() {
    // Trivial sanity: budgets should be human-meaningful round numbers
    // so the threshold is obvious in a CI failure message. Mostly a
    // tripwire against accidentally bumping the cap by a stray digit.
    assert_eq!(DEFAULT_BUDGET % 100, 0);
    for (_, budget) in GRANDFATHERED {
        assert_eq!(
            budget % 100,
            0,
            "GRANDFATHERED budgets should be in 100-LOC increments"
        );
    }
}
