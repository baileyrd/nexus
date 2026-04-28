//! Obsidian single-file `.base` (YAML) format — types and round-trip parsing.
//!
//! See ADR 0019. This is a parallel format to the Nexus `.bases` directory
//! (see `crate::bases`). Records are not stored on disk; they are computed
//! by querying the vault at view time. This module owns the *file* shape
//! only. Filter expression evaluation is in [`filter`]; the SQLite-backed
//! query that builds [`filter::NoteFacts`] from the index lives in
//! `nexus-storage`.

pub mod filter;

use serde::{Deserialize, Serialize};

// ── Types ────────────────────────────────────────────────────────────────────

/// A parsed Obsidian `.base` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObsidianBase {
    /// Top-level filter tree applied to every candidate note. Absent
    /// means "include every note in the vault."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<FilterNode>,

    /// Column / property configuration. Keys are property paths
    /// (`title`, `author`, `file.name`, `file.mtime`, …). Display
    /// metadata only — the actual values come from each note's
    /// frontmatter or file intrinsics.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub properties: serde_json::Map<String, serde_json::Value>,

    /// View definitions in the order they appear in the file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<ObsidianView>,
}

/// A single view inside an Obsidian `.base` file.
///
/// Mirrors `crate::bases::BaseView` where shapes overlap, but is kept
/// separate because the field semantics differ (e.g. `order` is the
/// visible-column list, not `fields`; `filters` here is a nested tree,
/// not a flat list of `FilterRule`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObsidianView {
    /// View display name.
    pub name: String,

    /// View type. Lowercase string matched against
    /// `table | cards | list | board | calendar | gallery | timeline`.
    /// Kept as a string (not an enum) so unknown types round-trip
    /// instead of erroring — Obsidian adds new view types over time.
    #[serde(rename = "type")]
    pub view_type: String,

    /// Visible properties in display order. Each entry is a property
    /// path resolvable in `ObsidianBase::properties`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,

    /// Per-view sort rules. Empty = inherit / unsorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<ObsidianSort>,

    /// Per-view filter tree. Combined with the file-level
    /// `ObsidianBase::filters` via logical AND at evaluation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<FilterNode>,

    /// Field used to group rows (board / list views).
    #[serde(rename = "groupBy", default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,

    /// Maximum rows to render. `None` = no cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Sort direction. Obsidian uses uppercase `ASC`/`DESC` in YAML.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SortDirection {
    /// Ascending order.
    #[default]
    Asc,
    /// Descending order.
    Desc,
}

/// One sort rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObsidianSort {
    /// Property path to sort by.
    pub property: String,
    /// Direction. Defaults to ascending.
    #[serde(default)]
    pub direction: SortDirection,
}

/// A boolean tree of filter expressions.
///
/// Obsidian represents this as a nested map with the keys `and`, `or`,
/// `not`, where the value is either a list of further nodes or a raw
/// expression string. The expression grammar itself is evaluated by
/// `nexus-storage::obsidian_base::filter` — at the type level we keep
/// expressions as opaque strings so unsupported expressions still
/// round-trip through serde.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterNode {
    /// All children must match.
    And {
        /// Child nodes to AND together.
        and: Vec<FilterNode>,
    },
    /// At least one child must match.
    Or {
        /// Child nodes to OR together.
        or: Vec<FilterNode>,
    },
    /// Negation of a single child.
    Not {
        /// Child node to negate.
        not: Box<FilterNode>,
    },
    /// A leaf expression, e.g. `'type == "literature"'`.
    Expr(String),
}

// ── Parse / Serialize ────────────────────────────────────────────────────────

/// Errors from parsing or serializing an Obsidian `.base` file.
#[derive(Debug, thiserror::Error)]
pub enum ObsidianBaseError {
    /// YAML parse failure.
    #[error("invalid .base YAML: {0}")]
    Parse(#[from] serde_yaml::Error),
}

/// Parse the YAML contents of a `.base` file.
///
/// # Errors
///
/// Returns [`ObsidianBaseError::Parse`] when the input is not valid YAML
/// or does not match the `.base` shape.
pub fn parse(contents: &str) -> Result<ObsidianBase, ObsidianBaseError> {
    let base: ObsidianBase = serde_yaml::from_str(contents)?;
    Ok(base)
}

/// Serialize an `ObsidianBase` back to YAML.
///
/// # Errors
///
/// Returns [`ObsidianBaseError::Parse`] if serialization fails — this
/// is unreachable in practice because every field is serde-derived.
pub fn to_yaml(base: &ObsidianBase) -> Result<String, ObsidianBaseError> {
    Ok(serde_yaml::to_string(base)?)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces `seed/17-reading.base` from the screenshot in the
    /// bug report — the canonical Obsidian "reading list" example.
    const READING_BASE: &str = r#"filters:
  and:
    - 'type == "literature"'
properties:
  file.name:
    displayName: Note
  title:
    displayName: Title
  author:
    displayName: Author
  year:
    displayName: Year
  rating:
    displayName: '*'
views:
  - type: table
    name: Library
    order:
      - title
      - author
      - year
      - rating
    sort:
      - property: year
        direction: DESC
  - type: cards
    name: Shelf
    order:
      - title
      - author
"#;

    #[test]
    fn parses_reading_fixture() {
        let base = parse(READING_BASE).expect("fixture must parse");

        // filters: and: ['type == "literature"']
        match base.filters.as_ref().expect("filters present") {
            FilterNode::And { and } => {
                assert_eq!(and.len(), 1);
                match &and[0] {
                    FilterNode::Expr(e) => assert_eq!(e, "type == \"literature\""),
                    other => panic!("expected leaf expr, got {other:?}"),
                }
            }
            other => panic!("expected And node, got {other:?}"),
        }

        // properties — five keys, each carrying a displayName.
        let prop_keys: Vec<&String> = base.properties.keys().collect();
        assert_eq!(prop_keys.len(), 5);
        for key in ["file.name", "title", "author", "year", "rating"] {
            assert!(base.properties.contains_key(key), "missing {key}");
        }
        assert_eq!(
            base.properties["title"]["displayName"]
                .as_str()
                .unwrap(),
            "Title"
        );
        assert_eq!(
            base.properties["rating"]["displayName"]
                .as_str()
                .unwrap(),
            "*"
        );

        // views — table "Library" then cards "Shelf".
        assert_eq!(base.views.len(), 2);
        let lib = &base.views[0];
        assert_eq!(lib.name, "Library");
        assert_eq!(lib.view_type, "table");
        assert_eq!(lib.order, vec!["title", "author", "year", "rating"]);
        assert_eq!(lib.sort.len(), 1);
        assert_eq!(lib.sort[0].property, "year");
        assert_eq!(lib.sort[0].direction, SortDirection::Desc);

        let shelf = &base.views[1];
        assert_eq!(shelf.name, "Shelf");
        assert_eq!(shelf.view_type, "cards");
        assert_eq!(shelf.order, vec!["title", "author"]);
    }

    #[test]
    fn round_trip_preserves_structure() {
        let parsed = parse(READING_BASE).unwrap();
        let yaml = to_yaml(&parsed).unwrap();
        let reparsed = parse(&yaml).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn empty_file_parses_to_default() {
        let base = parse("").unwrap_or_default();
        assert!(base.filters.is_none());
        assert!(base.properties.is_empty());
        assert!(base.views.is_empty());
    }

    #[test]
    fn no_filters_means_match_all() {
        let yaml = r#"properties: {}
views:
  - type: table
    name: All notes
"#;
        let base = parse(yaml).unwrap();
        assert!(base.filters.is_none());
        assert_eq!(base.views.len(), 1);
    }

    #[test]
    fn nested_or_and_not_round_trip() {
        let yaml = r#"filters:
  or:
    - 'type == "book"'
    - and:
        - 'type == "article"'
        - not: 'archived == true'
"#;
        let base = parse(yaml).unwrap();
        match base.filters.unwrap() {
            FilterNode::Or { or } => {
                assert_eq!(or.len(), 2);
                assert!(matches!(&or[0], FilterNode::Expr(e) if e == "type == \"book\""));
                match &or[1] {
                    FilterNode::And { and } => {
                        assert_eq!(and.len(), 2);
                        assert!(matches!(&and[1], FilterNode::Not { .. }));
                    }
                    other => panic!("expected And, got {other:?}"),
                }
            }
            other => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn unknown_view_type_round_trips_as_string() {
        let yaml = r#"views:
  - type: future-view-type-we-dont-know-yet
    name: Mystery
"#;
        let base = parse(yaml).unwrap();
        assert_eq!(base.views[0].view_type, "future-view-type-we-dont-know-yet");
        let again = to_yaml(&base).unwrap();
        assert!(again.contains("future-view-type-we-dont-know-yet"));
    }

    #[test]
    fn invalid_yaml_returns_parse_error() {
        let err = parse("filters:\n  and:\n  - [oops").unwrap_err();
        assert!(matches!(err, ObsidianBaseError::Parse(_)));
    }
}
