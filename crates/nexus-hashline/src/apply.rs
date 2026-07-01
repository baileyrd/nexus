//! Applying a parsed patch to file content, with stale-TAG 3-way merge.

use crate::error::HashlineError;
use crate::parse::{FileSection, Op};
use crate::snapshot::SnapshotStore;
use crate::tag::tag;

/// Outcome of applying one [`FileSection`] to a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    /// The TAG matched the live file; operations applied directly.
    Applied {
        /// The new file content.
        content: String,
    },
    /// The TAG was stale, but a 3-way merge against the recorded base succeeded.
    Merged {
        /// The merged file content.
        content: String,
    },
    /// The TAG was stale and the 3-way merge could not be resolved cleanly.
    Conflict {
        /// Merged content carrying diff3 conflict markers.
        markers: String,
    },
}

/// How a given original line is covered by an operation.
enum Cover {
    /// Line is the start of a swap; emit these replacement lines once here.
    Swap(Vec<String>),
    /// Line is deleted (or a non-start line of a swap range): emit nothing.
    Del,
}

/// Apply a section's operations against `current`, recovering from a stale TAG
/// via a 3-way merge when a base snapshot is available.
///
/// # Errors
///
/// * [`HashlineError::BlockOpsUnsupported`] if the section contains block ops.
/// * [`HashlineError::StaleTag`] if the TAG is stale and no base snapshot exists.
/// * Range/overlap/bounds errors from [`apply_ops`].
pub fn apply_section(
    section: &FileSection,
    current: &str,
    snapshots: &SnapshotStore,
) -> Result<EditOutcome, HashlineError> {
    if section.ops.iter().any(Op::is_block) {
        return Err(HashlineError::BlockOpsUnsupported);
    }

    let current_tag = tag(current);
    if section.tag.eq_ignore_ascii_case(&current_tag) {
        return Ok(EditOutcome::Applied {
            content: apply_ops(current, &section.ops)?,
        });
    }

    let Some(base) = snapshots.get_by_tag(&section.path, &section.tag) else {
        return Err(HashlineError::StaleTag {
            patch_tag: section.tag.clone(),
            current_tag,
        });
    };

    // base = what the author saw; ours = the author's intended result;
    // theirs = the file as it stands now.
    let ours = apply_ops(&base.content, &section.ops)?;
    match diffy::merge(&base.content, &ours, current) {
        Ok(content) => Ok(EditOutcome::Merged { content }),
        Err(markers) => Ok(EditOutcome::Conflict { markers }),
    }
}

/// Apply line/insert operations to `content`.
///
/// Operations address the *original* line numbering; they are reconstructed in a
/// single pass so indices never drift. Ranges must be disjoint.
///
/// # Errors
///
/// * [`HashlineError::BlockOpsUnsupported`] for any block operation.
/// * [`HashlineError::BadRange`] for an empty/inverted/zero range.
/// * [`HashlineError::LineOutOfBounds`] for an out-of-range line.
/// * [`HashlineError::OverlappingOps`] when two operations touch the same line.
pub fn apply_ops(content: &str, ops: &[Op]) -> Result<String, HashlineError> {
    let had_trailing_nl = content.ends_with('\n');
    let lines: Vec<&str> = content.lines().collect();
    let n = lines.len();

    let mut cover: Vec<Option<Cover>> = Vec::with_capacity(n);
    cover.resize_with(n, || None);
    let mut head: Vec<String> = Vec::new();
    let mut tail: Vec<String> = Vec::new();
    let mut pre: Vec<(usize, Vec<String>)> = Vec::new();
    let mut post: Vec<(usize, Vec<String>)> = Vec::new();

    for op in ops {
        match op {
            Op::InsHead { body } => head.extend(body.iter().cloned()),
            Op::InsTail { body } => tail.extend(body.iter().cloned()),
            Op::InsPre { line, body } => {
                check_line(*line, n)?;
                pre.push((*line, body.clone()));
            }
            Op::InsPost { line, body } => {
                check_line(*line, n)?;
                post.push((*line, body.clone()));
            }
            Op::Swap { start, end, body } => {
                check_range(*start, *end, n)?;
                claim(&mut cover, *start, *end)?;
                cover[*start - 1] = Some(Cover::Swap(body.clone()));
            }
            Op::Del { start, end } => {
                check_range(*start, *end, n)?;
                claim(&mut cover, *start, *end)?;
            }
            Op::SwapBlock { .. } | Op::DelBlock { .. } | Op::InsBlockPost { .. } => {
                return Err(HashlineError::BlockOpsUnsupported);
            }
        }
    }

    let mut out: Vec<String> = head;
    for (i, original) in lines.iter().enumerate() {
        let line = i + 1;
        for (anchor, body) in &pre {
            if *anchor == line {
                out.extend(body.iter().cloned());
            }
        }
        match &cover[i] {
            None => out.push((*original).to_string()),
            Some(Cover::Swap(body)) => out.extend(body.iter().cloned()),
            Some(Cover::Del) => {}
        }
        for (anchor, body) in &post {
            if *anchor == line {
                out.extend(body.iter().cloned());
            }
        }
    }
    out.extend(tail);

    let mut result = out.join("\n");
    if had_trailing_nl && !result.is_empty() {
        result.push('\n');
    }
    Ok(result)
}

/// Mark lines `start..=end` as covered, erroring on overlap. The whole range is
/// claimed as [`Cover::Del`]; a swap then overwrites its start line.
fn claim(cover: &mut [Option<Cover>], start: usize, end: usize) -> Result<(), HashlineError> {
    for line in start..=end {
        if cover[line - 1].is_some() {
            return Err(HashlineError::OverlappingOps { line });
        }
        cover[line - 1] = Some(Cover::Del);
    }
    Ok(())
}

fn check_line(line: usize, n: usize) -> Result<(), HashlineError> {
    if line == 0 || line > n {
        Err(HashlineError::LineOutOfBounds { line, len: n })
    } else {
        Ok(())
    }
}

fn check_range(start: usize, end: usize, n: usize) -> Result<(), HashlineError> {
    if start == 0 || end < start {
        return Err(HashlineError::BadRange { start, end });
    }
    if end > n {
        return Err(HashlineError::LineOutOfBounds { line: end, len: n });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::snapshot::SnapshotStore;

    fn ops(patch: &str) -> Vec<Op> {
        parse(patch)
            .unwrap()
            .sections
            .into_iter()
            .next()
            .unwrap()
            .ops
    }

    #[test]
    fn swap_replaces_inclusive_range() {
        let out = apply_ops("a\nb\nc\nd\n", &ops("[f#ABCD]\nSWAP 2.=3:\n+B\n+C\n+C2\n")).unwrap();
        assert_eq!(out, "a\nB\nC\nC2\nd\n");
    }

    #[test]
    fn del_removes_range_and_preserves_trailing_newline() {
        let out = apply_ops("a\nb\nc\n", &ops("[f#ABCD]\nDEL 2.=2\n")).unwrap();
        assert_eq!(out, "a\nc\n");
    }

    #[test]
    fn no_trailing_newline_is_preserved() {
        let out = apply_ops("a\nb", &ops("[f#ABCD]\nSWAP 1.=1:\n+A\n")).unwrap();
        assert_eq!(out, "A\nb");
    }

    #[test]
    fn inserts_pre_post_head_tail() {
        let out = apply_ops(
            "x\ny\n",
            &ops("[f#ABCD]\nINS.HEAD:\n+top\nINS.PRE 2:\n+beforeY\nINS.POST 2:\n+afterY\nINS.TAIL:\n+bottom\n"),
        )
        .unwrap();
        assert_eq!(out, "top\nx\nbeforeY\ny\nafterY\nbottom\n");
    }

    #[test]
    fn insert_into_empty_file() {
        let out = apply_ops("", &ops("[f#ABCD]\nINS.HEAD:\n+hello\nINS.TAIL:\n+world\n")).unwrap();
        assert_eq!(out, "hello\nworld");
    }

    #[test]
    fn multiple_disjoint_ops_compose() {
        let out = apply_ops(
            "1\n2\n3\n4\n5\n",
            &ops("[f#ABCD]\nSWAP 1.=1:\n+ONE\nDEL 3.=3\nINS.POST 5:\n+SIX\n"),
        )
        .unwrap();
        assert_eq!(out, "ONE\n2\n4\n5\nSIX\n");
    }

    #[test]
    fn overlapping_ops_error() {
        let err = apply_ops("a\nb\nc\n", &ops("[f#ABCD]\nSWAP 1.=2:\n+X\nDEL 2.=2\n")).unwrap_err();
        assert_eq!(err, HashlineError::OverlappingOps { line: 2 });
    }

    #[test]
    fn out_of_bounds_and_bad_range_error() {
        assert_eq!(
            apply_ops("a\n", &ops("[f#ABCD]\nDEL 5.=5\n")).unwrap_err(),
            HashlineError::LineOutOfBounds { line: 5, len: 1 }
        );
        assert_eq!(
            apply_ops("a\nb\n", &ops("[f#ABCD]\nSWAP 2.=1:\n+x\n")).unwrap_err(),
            HashlineError::BadRange { start: 2, end: 1 }
        );
    }

    #[test]
    fn apply_section_applies_on_matching_tag() {
        let current = "a\nb\n";
        let t = tag(current);
        let section = parse(&format!("[f#{t}]\nSWAP 1.=1:\n+A\n"))
            .unwrap()
            .sections
            .remove(0);
        let outcome = apply_section(&section, current, &SnapshotStore::new()).unwrap();
        assert_eq!(
            outcome,
            EditOutcome::Applied {
                content: "A\nb\n".to_string()
            }
        );
    }

    #[test]
    fn apply_section_three_way_merges_on_stale_tag() {
        // Two edits separated by an unchanged context line merge cleanly.
        let base = "a\nb\nc\nd\ne\n";
        let base_tag = tag(base);
        // The author patched against `base`, replacing line 2 (`b`).
        let section = parse(&format!("[f#{base_tag}]\nSWAP 2.=2:\n+b-edited\n"))
            .unwrap()
            .sections
            .remove(0);
        // Meanwhile the file changed line 4 (`d`), with `c` unchanged between.
        let current = "a\nb\nc\nd-changed\ne\n";
        let mut store = SnapshotStore::new();
        store.record("f", base);
        let outcome = apply_section(&section, current, &store).unwrap();
        assert_eq!(
            outcome,
            EditOutcome::Merged {
                content: "a\nb-edited\nc\nd-changed\ne\n".to_string()
            }
        );
    }

    #[test]
    fn apply_section_reports_conflict() {
        let base = "shared\n";
        let base_tag = tag(base);
        let section = parse(&format!("[f#{base_tag}]\nSWAP 1.=1:\n+ours\n"))
            .unwrap()
            .sections
            .remove(0);
        let current = "theirs\n"; // both sides changed the same line
        let mut store = SnapshotStore::new();
        store.record("f", base);
        let outcome = apply_section(&section, current, &store).unwrap();
        assert!(matches!(outcome, EditOutcome::Conflict { .. }));
    }

    #[test]
    fn apply_section_stale_without_snapshot_errors() {
        let section = parse("[f#0000]\nSWAP 1.=1:\n+x\n")
            .unwrap()
            .sections
            .remove(0);
        let err = apply_section(&section, "different\n", &SnapshotStore::new()).unwrap_err();
        assert!(matches!(err, HashlineError::StaleTag { .. }));
    }

    #[test]
    fn apply_section_rejects_block_ops() {
        let current = "a\n";
        let t = tag(current);
        let section = parse(&format!("[f#{t}]\nSWAP.BLK 1:\n+x\n"))
            .unwrap()
            .sections
            .remove(0);
        assert_eq!(
            apply_section(&section, current, &SnapshotStore::new()).unwrap_err(),
            HashlineError::BlockOpsUnsupported
        );
    }
}
