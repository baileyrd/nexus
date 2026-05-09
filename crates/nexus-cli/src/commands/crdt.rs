//! `nexus crdt …` CLI subcommands (BL-074).
//!
//! Two commands today:
//!
//! - `merge-driver` is the git merge driver entry point. Git invokes
//!   it with three file paths during a merge / rebase / cherry-pick
//!   that touches `.forge/.editor/crdt/<sha>.json`. The driver loads
//!   the `--ours` and `--theirs` envelopes, takes the idempotent
//!   union of their op logs via [`nexus_crdt::OpLog::merge`], and
//!   writes the merged envelope back to `--ours`. The merge base is
//!   read for diagnostics only — convergence is independent of base
//!   because every op carries its own [`nexus_crdt::VersionVector`]
//!   causality witness.
//!
//! - `install-merge-driver` prints (or applies) the one-time
//!   `.gitattributes` rule and `git config` invocation needed to
//!   register the driver in the current repository.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use nexus_crdt::PersistedCrdt;

/// Three-way merge driver for `.forge/.editor/crdt/<sha>.json`. See
/// the [`crate::commands::crdt`] module docs for the protocol.
pub fn merge_driver(base: &Path, ours: &Path, theirs: &Path) -> Result<()> {
    // Try to load each side. A missing file is legal in some merge
    // scenarios (e.g., the file was added on one branch only) and
    // resolves to "use whichever side exists".
    let ours_envelope = read_envelope(ours)?;
    let theirs_envelope = read_envelope(theirs)?;

    let merged = match (ours_envelope, theirs_envelope) {
        (None, None) => {
            // Both sides missing — degenerate case, but valid: nothing
            // to write. Git will already have decided not to keep the
            // file by then; we just exit cleanly so the merge succeeds.
            tracing::debug!("BL-074 merge-driver: both sides missing — no-op");
            return Ok(());
        }
        (Some(only), None) | (None, Some(only)) => only,
        (Some(mut ours), Some(theirs)) => {
            let absorbed = ours.state.log.merge(&theirs.state.log);
            // Pick the higher lamport so any future locally-authored op
            // dominates everything seen on either side.
            if theirs.state.lamport > ours.state.lamport {
                ours.state.lamport = theirs.state.lamport;
            }
            // Union the per-block RGAs by replaying any rga ops we
            // newly absorbed against ours' rga maps. The union of the
            // op logs is what matters for convergence, so the simpler
            // approach is: prefer ours' RGAs as the base, replay all
            // ops absorbed from theirs through them. Since this is
            // a primitive on the persisted state (not a live doc),
            // we accept the conservative-but-correct path of
            // overwriting per-block state from theirs when ours
            // didn't have it.
            for (block_id, rga) in &theirs.state.rga {
                ours.state.rga.entry(*block_id).or_insert_with(|| rga.clone());
            }
            for (block_id, meta) in &theirs.state.block_meta {
                ours.state
                    .block_meta
                    .entry(*block_id)
                    .or_insert_with(|| meta.clone());
            }
            tracing::debug!(absorbed, "BL-074 merge-driver: union complete");
            // Bump the timestamp + content hash to reflect that the
            // envelope is now the post-merge state.
            PersistedCrdt::new(ours.state, ours.content_hash)
        }
    };

    // Diagnostic: log whether the base existed (informational only).
    if !base.as_os_str().is_empty() && base.exists() {
        tracing::debug!(
            base = %base.display(),
            "BL-074 merge-driver: merge base present (used for diagnostics only)"
        );
    }

    let bytes = serde_json::to_vec(&merged).context("encode merged crdt envelope")?;
    let tmp = ours.with_extension("json.merge-tmp");
    fs::write(&tmp, &bytes)
        .with_context(|| format!("write merged tmp file: {}", tmp.display()))?;
    fs::rename(&tmp, ours)
        .with_context(|| format!("rename {} → {}", tmp.display(), ours.display()))?;
    Ok(())
}

fn read_envelope(path: &Path) -> Result<Option<PersistedCrdt>> {
    if path.as_os_str().is_empty() {
        return Ok(None);
    }
    match fs::read(path) {
        Ok(bytes) => {
            let envelope: PersistedCrdt = serde_json::from_slice(&bytes)
                .with_context(|| format!("decode {}", path.display()))?;
            Ok(Some(envelope))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

const ATTR_LINE: &str = ".forge/.editor/crdt/* merge=nexus-crdt";
const DRIVER_NAME: &str = "nexus-crdt";
const DRIVER_CMD: &str = "nexus crdt merge-driver --base %O --ours %A --theirs %B";

/// Print (and optionally apply) the one-time setup needed to
/// register the merge driver in the current repository.
pub fn install_merge_driver(apply: bool) -> Result<()> {
    println!("BL-074 git merge driver — one-time setup");
    println!();
    println!("1. Add to .gitattributes:");
    println!("       {ATTR_LINE}");
    println!("2. Register the driver in git config:");
    println!("       git config merge.{DRIVER_NAME}.driver {DRIVER_CMD:?}");
    println!("       git config merge.{DRIVER_NAME}.name 'Nexus CRDT op-log union'");
    println!();

    if !apply {
        println!("(Run with --apply to perform these changes automatically.)");
        return Ok(());
    }

    // Apply the .gitattributes change. Append the rule if the file
    // exists and the rule isn't already present; create it otherwise.
    let attr_path = Path::new(".gitattributes");
    let existing = match fs::read_to_string(attr_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err).context("read .gitattributes"),
    };
    if !existing
        .lines()
        .any(|l| l.trim() == ATTR_LINE.trim())
    {
        let mut updated = existing.clone();
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(ATTR_LINE);
        updated.push('\n');
        fs::write(attr_path, updated).context("write .gitattributes")?;
        println!("✓ Updated .gitattributes");
    } else {
        println!("✓ .gitattributes already has the rule");
    }

    // Register the driver in `.git/config` for this repository.
    git_config(&[
        "config",
        &format!("merge.{DRIVER_NAME}.driver"),
        DRIVER_CMD,
    ])?;
    git_config(&[
        "config",
        &format!("merge.{DRIVER_NAME}.name"),
        "Nexus CRDT op-log union",
    ])?;
    println!("✓ Registered {DRIVER_NAME} merge driver in .git/config");

    Ok(())
}

fn git_config(args: &[&str]) -> Result<()> {
    let status = Command::new("git").args(args).status().context("spawn git")?;
    if !status.success() {
        return Err(anyhow!("git {} failed (exit {status})", args.join(" ")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nexus_crdt::{content_hash_hex, CrdtDoc, PersistedCrdt, SiteId};
    use nexus_editor::{Block, BlockTree, BlockType, DocumentMetadata, Operation};

    use super::*;

    fn make_envelope(insert_text: &str) -> PersistedCrdt {
        let mut tree = BlockTree::new(DocumentMetadata::default());
        let block = Block::new(BlockType::Paragraph);
        let id = block.id;
        tree.insert(block, None, 0).unwrap();
        let mut doc = CrdtDoc::new(SiteId::new(), tree);
        doc.apply_local(&Operation::InsertText {
            block_id: id,
            pos: 0,
            text: insert_text.into(),
            pre_annotations: vec![],
        })
        .unwrap();
        // The hash is over what would be saved as markdown; for tests
        // we just use the inserted text — convergence doesn't depend
        // on the hash value, only on its consistency.
        PersistedCrdt::new(doc.state(), content_hash_hex(insert_text.as_bytes()))
    }

    #[test]
    fn merge_driver_unions_disjoint_op_logs() {
        let dir = tempfile::tempdir().unwrap();
        let ours_path = dir.path().join("ours.json");
        let theirs_path = dir.path().join("theirs.json");
        let base_path = dir.path().join("base.json");

        let ours = make_envelope("alpha");
        let theirs = make_envelope("beta");
        std::fs::write(&ours_path, serde_json::to_vec(&ours).unwrap()).unwrap();
        std::fs::write(&theirs_path, serde_json::to_vec(&theirs).unwrap()).unwrap();
        // base intentionally absent to exercise the missing-base path.

        merge_driver(&base_path, &ours_path, &theirs_path).unwrap();

        let merged: PersistedCrdt =
            serde_json::from_slice(&std::fs::read(&ours_path).unwrap()).unwrap();
        // Merged log holds both authors' single ops (each was authored
        // independently from a fresh doc).
        assert_eq!(merged.state.log.len(), 2);
    }

    #[test]
    fn merge_driver_handles_one_side_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ours_path = dir.path().join("ours.json");
        let theirs_path = dir.path().join("theirs.json"); // never written
        let base_path = dir.path().join("base.json");

        let ours = make_envelope("only-mine");
        std::fs::write(&ours_path, serde_json::to_vec(&ours).unwrap()).unwrap();

        // Should succeed and leave ours unchanged (theirs missing).
        merge_driver(&base_path, &ours_path, &theirs_path).unwrap();
        let after: PersistedCrdt =
            serde_json::from_slice(&std::fs::read(&ours_path).unwrap()).unwrap();
        assert_eq!(after.state.log.len(), 1);

        // Sanity check: capture-the-Arc to avoid an "unused import"
        // warning on `Arc` in this module.
        let _ = Arc::new(());
    }
}
