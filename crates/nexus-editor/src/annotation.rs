//! Inline formatting annotations over a block's plain-text content
//! (PRD 08 §2).
//!
//! Annotations are non-destructive overlays on top of plain text —
//! they carry `(start, end)` byte ranges plus a type/payload, and the
//! UI renderer composes them into the final styled output.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::block::{BlockId, PropertyValue};

// ── Annotation ────────────────────────────────────────────────────────────────

/// A single inline formatting range over a block's `content` string.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    /// Inclusive start byte within `Block::content`.
    pub start: usize,
    /// Exclusive end byte within `Block::content`.
    pub end: usize,
    /// Type + payload.
    pub ty: AnnotationType,
}

impl Annotation {
    /// `true` if `self` and `other` share at least one byte.
    ///
    /// Touching ranges (e.g. `0..3` and `3..5`) do **not** overlap.
    #[must_use]
    pub fn overlaps(&self, other: &Annotation) -> bool {
        !(self.end <= other.start || self.start >= other.end)
    }

    /// `true` once the range has collapsed to zero length.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

// ── AnnotationType ────────────────────────────────────────────────────────────

/// Discriminant + payload for every supported inline annotation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AnnotationType {
    /// Bold.
    Bold,
    /// Italic.
    Italic,
    /// Strikethrough.
    Strikethrough,
    /// Underline.
    Underline,
    /// Inline code (monospace).
    Code,

    /// Foreground text color token or CSS value.
    TextColor(String),
    /// Background highlight color token or CSS value.
    HighlightColor(String),

    /// External or local hyperlink.
    Link {
        /// Target URL.
        url: String,
        /// Optional title attribute (tooltip).
        title: Option<String>,
    },

    /// Internal wiki-style link.
    Wikilink {
        /// Target path or name.
        path: String,
        /// Optional display override.
        display_text: Option<String>,
        /// Whether the target resolves to a real file.
        is_resolved: bool,
    },

    /// Mention of a user or entity.
    Mention {
        /// Stable user id.
        user_id: String,
        /// Display label.
        display_name: String,
    },

    /// Inline LaTeX math (`$...$`).
    MathInline {
        /// LaTeX source.
        formula: String,
    },

    /// Reference to a block inside the current document.
    BlockRef {
        /// Referenced block.
        block_id: BlockId,
    },

    /// Plugin-supplied custom annotation.
    Custom {
        /// Originating plugin ID.
        plugin_id: String,
        /// Plugin-local type discriminant.
        ty: String,
        /// Arbitrary payload.
        data: HashMap<String, PropertyValue>,
    },
}

// ── Functions ─────────────────────────────────────────────────────────────────

/// Merge adjacent or overlapping annotations whose payloads match.
///
/// Sort order is stable on `(start, end)`. For annotations with payloads
/// (Link, Wikilink, Mention, etc.), full equality is required before
/// merging: two `Link`s with different URLs are preserved as separate
/// ranges.
#[must_use]
pub fn merge(mut annotations: Vec<Annotation>) -> Vec<Annotation> {
    annotations.sort_by_key(|a| (a.start, a.end));

    let mut iter = annotations.into_iter();
    let Some(first) = iter.next() else {
        return Vec::new();
    };
    let mut out = vec![first];
    for ann in iter {
        let Some(last) = out.last_mut() else { break };
        if last.end >= ann.start && last.ty == ann.ty {
            last.end = last.end.max(ann.end);
        } else {
            out.push(ann);
        }
    }
    out
}

/// Shift `annotations` to reflect a text edit at `edit_start`.
///
/// `edit_length` is positive for insertions and negative for deletions
/// (where `|edit_length|` is the number of bytes removed starting at
/// `edit_start`).
///
/// Conventions at boundaries:
/// - Insert at `ann.start`: the annotation is pushed right (the
///   inserted text is treated as preceding the annotation).
/// - Insert at `ann.end`: no change (insertion is outside).
/// - Delete that fully covers the annotation: the annotation collapses
///   to `start == end` at the delete position.
pub fn adjust_annotations(annotations: &mut [Annotation], edit_start: usize, edit_length: isize) {
    if edit_length == 0 {
        return;
    }

    if edit_length > 0 {
        let len = edit_length.unsigned_abs();
        for ann in annotations.iter_mut() {
            if edit_start <= ann.start {
                ann.start = ann.start.saturating_add(len);
                ann.end = ann.end.saturating_add(len);
            } else if edit_start < ann.end {
                ann.end = ann.end.saturating_add(len);
            }
        }
        return;
    }

    // Deletion branch.
    let len = edit_length.unsigned_abs();
    let del_start = edit_start;
    let del_end = edit_start.saturating_add(len);
    for ann in annotations.iter_mut() {
        ann.start = map_through_delete(ann.start, del_start, del_end, len);
        ann.end = map_through_delete(ann.end, del_start, del_end, len);
    }
}

fn map_through_delete(p: usize, del_start: usize, del_end: usize, len: usize) -> usize {
    if p <= del_start {
        p
    } else if p >= del_end {
        p.saturating_sub(len)
    } else {
        del_start
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ann(start: usize, end: usize, ty: AnnotationType) -> Annotation {
        Annotation { start, end, ty }
    }

    // ── overlaps ──

    #[test]
    fn overlaps_disjoint_is_false() {
        assert!(!ann(0, 3, AnnotationType::Bold).overlaps(&ann(5, 8, AnnotationType::Bold)));
    }

    #[test]
    fn overlaps_touching_is_false() {
        assert!(!ann(0, 3, AnnotationType::Bold).overlaps(&ann(3, 5, AnnotationType::Bold)));
    }

    #[test]
    fn overlaps_partial_is_true() {
        assert!(ann(0, 5, AnnotationType::Bold).overlaps(&ann(3, 8, AnnotationType::Bold)));
    }

    #[test]
    fn overlaps_contained_is_true() {
        assert!(ann(0, 10, AnnotationType::Bold).overlaps(&ann(3, 7, AnnotationType::Bold)));
    }

    // ── merge ──

    #[test]
    fn merge_empty() {
        assert!(merge(vec![]).is_empty());
    }

    #[test]
    fn merge_adjacent_same_type() {
        let merged = merge(vec![
            ann(0, 3, AnnotationType::Bold),
            ann(3, 6, AnnotationType::Bold),
        ]);
        assert_eq!(merged, vec![ann(0, 6, AnnotationType::Bold)]);
    }

    #[test]
    fn merge_overlapping_same_type() {
        let merged = merge(vec![
            ann(0, 5, AnnotationType::Bold),
            ann(3, 7, AnnotationType::Bold),
        ]);
        assert_eq!(merged, vec![ann(0, 7, AnnotationType::Bold)]);
    }

    #[test]
    fn merge_different_types_kept_separate() {
        let merged = merge(vec![
            ann(0, 3, AnnotationType::Bold),
            ann(3, 6, AnnotationType::Italic),
        ]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_same_link_url_merges() {
        let link = AnnotationType::Link {
            url: "https://x".into(),
            title: None,
        };
        let merged = merge(vec![ann(0, 3, link.clone()), ann(3, 6, link.clone())]);
        assert_eq!(merged, vec![ann(0, 6, link)]);
    }

    #[test]
    fn merge_different_link_url_kept_separate() {
        let merged = merge(vec![
            ann(
                0,
                3,
                AnnotationType::Link {
                    url: "https://a".into(),
                    title: None,
                },
            ),
            ann(
                3,
                6,
                AnnotationType::Link {
                    url: "https://b".into(),
                    title: None,
                },
            ),
        ]);
        assert_eq!(merged.len(), 2);
    }

    // ── adjust_annotations — inserts ──

    #[test]
    fn adjust_insert_before_shifts_both() {
        let mut anns = vec![ann(5, 10, AnnotationType::Bold)];
        adjust_annotations(&mut anns, 2, 3);
        assert_eq!(anns[0].start, 8);
        assert_eq!(anns[0].end, 13);
    }

    #[test]
    fn adjust_insert_at_start_boundary_shifts_both() {
        let mut anns = vec![ann(5, 10, AnnotationType::Bold)];
        adjust_annotations(&mut anns, 5, 3);
        assert_eq!(anns[0].start, 8);
        assert_eq!(anns[0].end, 13);
    }

    #[test]
    fn adjust_insert_inside_extends_end() {
        let mut anns = vec![ann(5, 10, AnnotationType::Bold)];
        adjust_annotations(&mut anns, 7, 3);
        assert_eq!(anns[0].start, 5);
        assert_eq!(anns[0].end, 13);
    }

    #[test]
    fn adjust_insert_at_end_boundary_no_change() {
        let mut anns = vec![ann(5, 10, AnnotationType::Bold)];
        adjust_annotations(&mut anns, 10, 3);
        assert_eq!(anns[0].start, 5);
        assert_eq!(anns[0].end, 10);
    }

    #[test]
    fn adjust_insert_after_no_change() {
        let mut anns = vec![ann(5, 10, AnnotationType::Bold)];
        adjust_annotations(&mut anns, 12, 3);
        assert_eq!(anns[0], ann(5, 10, AnnotationType::Bold));
    }

    // ── adjust_annotations — deletes ──

    #[test]
    fn adjust_delete_before_shifts_both() {
        let mut anns = vec![ann(10, 15, AnnotationType::Bold)];
        adjust_annotations(&mut anns, 0, -3);
        assert_eq!(anns[0], ann(7, 12, AnnotationType::Bold));
    }

    #[test]
    fn adjust_delete_across_left_edge_trims_start() {
        let mut anns = vec![ann(10, 20, AnnotationType::Bold)];
        // Delete [8, 13): left edge crosses annotation start.
        adjust_annotations(&mut anns, 8, -5);
        // Annotation starts where the delete started (in new coords = 8),
        // and its end shifts by -5 = 15.
        assert_eq!(anns[0], ann(8, 15, AnnotationType::Bold));
    }

    #[test]
    fn adjust_delete_inside_shrinks_end() {
        let mut anns = vec![ann(5, 20, AnnotationType::Bold)];
        // Delete [8, 12): entirely inside.
        adjust_annotations(&mut anns, 8, -4);
        assert_eq!(anns[0], ann(5, 16, AnnotationType::Bold));
    }

    #[test]
    fn adjust_delete_across_right_edge_trims_end() {
        let mut anns = vec![ann(5, 15, AnnotationType::Bold)];
        // Delete [12, 20): right edge crosses annotation end.
        adjust_annotations(&mut anns, 12, -8);
        assert_eq!(anns[0], ann(5, 12, AnnotationType::Bold));
    }

    #[test]
    fn adjust_delete_covering_entire_annotation_collapses() {
        let mut anns = vec![ann(5, 10, AnnotationType::Bold)];
        // Delete [3, 12): wholly covers.
        adjust_annotations(&mut anns, 3, -9);
        assert_eq!(anns[0].start, anns[0].end);
        assert!(anns[0].is_empty());
    }

    #[test]
    fn adjust_delete_after_no_change() {
        let mut anns = vec![ann(5, 10, AnnotationType::Bold)];
        adjust_annotations(&mut anns, 12, -3);
        assert_eq!(anns[0], ann(5, 10, AnnotationType::Bold));
    }

    #[test]
    fn adjust_zero_length_no_op() {
        let mut anns = vec![ann(5, 10, AnnotationType::Bold)];
        adjust_annotations(&mut anns, 0, 0);
        assert_eq!(anns[0], ann(5, 10, AnnotationType::Bold));
    }
}
