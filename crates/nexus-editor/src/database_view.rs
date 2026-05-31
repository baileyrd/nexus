//! Database view executor — runs an inline `[[{db:query}]]` block's
//! [`DatabaseViewConfig`] through `com.nexus.database::apply_view`.
//!
//! This is split 1 of BL-012 (PRD-08 §8.1, `Block::DatabaseView`):
//! the pure executor + IPC surface. The CM6 widget, decoration
//! plumbing, undo integration, and filter/sort UX layer on top in
//! later splits.
//!
//! ## Responsibilities
//!
//! 1. Translate the editor-side [`DatabaseViewConfig`] (which stores
//!    filters / sorts as user-typed strings to keep the markdown
//!    block round-trippable) into the structured
//!    [`nexus_types::bases::BaseView`] that
//!    [`nexus_database::views::apply_view`] consumes.
//! 2. Resolve the target `.bases` directory through
//!    `com.nexus.storage::base_load` so the editor stays out of the
//!    fs layer.
//! 3. Hand schema + records to `com.nexus.database::apply_view` and
//!    return both the [`AppliedView`](nexus_types_bases_view) and the
//!    schema (the renderer needs field types for cell formatting).
//!
//! Filter strings follow the same `field <op> value` syntax as
//! `nexus-storage`'s `parse_filter`, but are translated into
//! `apply_view`'s string-tagged `FilterRule.operator` so the editor
//! crate can stay independent of `nexus-storage` / `nexus-database`
//! internals.

use nexus_types::bases::{BaseSchema, BaseView, FilterRule, SortRule, ViewType};
use serde::{Deserialize, Serialize};

use crate::block::{DatabaseViewConfig, DatabaseViewType};

/// Errors produced when translating a [`DatabaseViewConfig`] into a
/// [`BaseView`] suitable for `apply_view`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslateError {
    /// Filter string did not contain a recognised operator.
    UnrecognisedFilter(String),
    /// Filter string was missing the field name on the left of the
    /// operator.
    EmptyFilterField(String),
    /// Sort string did not contain a single field plus optional
    /// direction.
    InvalidSort(String),
    /// Sort direction was neither `asc` nor `desc`.
    InvalidSortDirection(String),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnrecognisedFilter(s) => {
                write!(f, "unrecognised filter expression: '{s}'")
            }
            Self::EmptyFilterField(s) => write!(f, "filter has empty field: '{s}'"),
            Self::InvalidSort(s) => write!(f, "invalid sort expression: '{s}'"),
            Self::InvalidSortDirection(s) => write!(f, "invalid sort direction: '{s}'"),
        }
    }
}

impl std::error::Error for TranslateError {}

/// Translate a [`DatabaseViewConfig`] (editor-side, string filters)
/// into a [`BaseView`] (database-side, structured rules) that
/// [`nexus_database::views::apply_view`] can consume.
///
/// `name` is stamped onto [`BaseView::name`]. The editor block does
/// not carry a view name today, so callers should pass a stable
/// placeholder (typically the empty string or the block id).
///
/// # Errors
///
/// Returns [`TranslateError`] if any filter or sort entry is
/// malformed.
pub fn config_to_view(name: &str, config: &DatabaseViewConfig) -> Result<BaseView, TranslateError> {
    let (view_type, group_field, date_field) = match &config.view_type {
        DatabaseViewType::Kanban { column_by } => (ViewType::Kanban, Some(column_by.clone()), None),
        DatabaseViewType::Calendar { date_field } => {
            (ViewType::Calendar, None, Some(date_field.clone()))
        }
        DatabaseViewType::Gallery { title_field: _ } => (ViewType::Gallery, None, None),
        // `Custom` is plugin-provided and has no native apply_view
        // mapping yet — fall back to the same flat-table treatment as
        // the default view.
        DatabaseViewType::Table | DatabaseViewType::Custom(_) => (ViewType::Table, None, None),
    };

    // Fall back to `group_by` for layouts that can group but didn't
    // receive a layout-specific column (Table doesn't group at all
    // — `apply_view` ignores `group_field` for `ViewType::Table`).
    let group_field = group_field.or_else(|| config.group_by.clone());

    let filter = config
        .filters
        .iter()
        .map(|s| parse_filter_string(s))
        .collect::<Result<Vec<_>, _>>()?;

    let sort = config
        .sorts
        .iter()
        .map(|s| parse_sort_string(s))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(BaseView {
        name: name.to_string(),
        view_type,
        fields: Vec::new(),
        sort,
        filter,
        group_field,
        date_field,
        end_field: None,
    })
}

/// Parse a filter string of the form `field <op> value` into a
/// [`FilterRule`] suitable for
/// [`nexus_database::views::apply_view`].
///
/// Operator vocabulary (mirrors `nexus_database`'s `matches_filter`):
///
/// * symbolic — `=`, `!=`, `>=`, `<=`, `>`, `<`
/// * word — `contains`, `icontains`, `starts_with`, `ends_with`
/// * suffix — `is_empty`, `is_not_empty`
///
/// Values are auto-typed: `true` / `false` → bool, integers / floats
/// → numbers, anything else → string. Wrap in single or double
/// quotes to force the literal-string interpretation.
///
/// # Errors
///
/// Returns [`TranslateError::UnrecognisedFilter`] when no operator
/// matches and [`TranslateError::EmptyFilterField`] when the field
/// name is empty.
pub fn parse_filter_string(input: &str) -> Result<FilterRule, TranslateError> {
    let trimmed = input.trim();

    // Suffix operators — `field is_empty` / `field is_not_empty`.
    for (suffix, op) in [(" is_not_empty", "is_not_empty"), (" is_empty", "is_empty")] {
        if let Some(field) = trimmed.strip_suffix(suffix) {
            let field = field.trim();
            if field.is_empty() {
                return Err(TranslateError::EmptyFilterField(input.to_string()));
            }
            return Ok(FilterRule {
                field: field.to_string(),
                operator: op.to_string(),
                value: serde_json::Value::Null,
            });
        }
    }

    // Word operators — match on space-padded form so a field like
    // `contains_pii` doesn't get clipped.
    for word in ["icontains", "contains", "starts_with", "ends_with"] {
        let pattern = format!(" {word} ");
        if let Some(pos) = trimmed.find(&pattern) {
            let field = trimmed[..pos].trim();
            if field.is_empty() {
                return Err(TranslateError::EmptyFilterField(input.to_string()));
            }
            let rest = trimmed[pos + pattern.len()..].trim();
            return Ok(FilterRule {
                field: field.to_string(),
                operator: word.to_string(),
                value: parse_value(rest),
            });
        }
    }

    // Symbolic operators — longest first so `>=` beats `>`.
    for (symbol, op) in [
        ("!=", "neq"),
        (">=", "gte"),
        ("<=", "lte"),
        (">", "gt"),
        ("<", "lt"),
        ("=", "eq"),
    ] {
        if let Some(pos) = trimmed.find(symbol) {
            let field = trimmed[..pos].trim();
            let rest = trimmed[pos + symbol.len()..].trim();
            if field.is_empty() {
                return Err(TranslateError::EmptyFilterField(input.to_string()));
            }
            return Ok(FilterRule {
                field: field.to_string(),
                operator: op.to_string(),
                value: parse_value(rest),
            });
        }
    }

    Err(TranslateError::UnrecognisedFilter(input.to_string()))
}

/// Parse a sort string like `"due_date asc"` into a [`SortRule`].
/// Direction defaults to `asc` when omitted.
///
/// # Errors
///
/// Returns [`TranslateError::InvalidSort`] when the string is empty
/// or has more than two whitespace-separated tokens, or
/// [`TranslateError::InvalidSortDirection`] when the second token is
/// not `asc` / `desc`.
pub fn parse_sort_string(input: &str) -> Result<SortRule, TranslateError> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let (field, direction) = match parts.as_slice() {
        [field] => ((*field).to_string(), "asc".to_string()),
        [field, dir] => {
            let lower = dir.to_lowercase();
            if lower != "asc" && lower != "desc" {
                return Err(TranslateError::InvalidSortDirection((*dir).to_string()));
            }
            ((*field).to_string(), lower)
        }
        _ => return Err(TranslateError::InvalidSort(input.to_string())),
    };
    Ok(SortRule { field, direction })
}

/// Coerce a raw value token into a JSON value. Strips matched outer
/// quotes, then tries bool → number → string in that order.
fn parse_value(s: &str) -> serde_json::Value {
    let s = s.trim();
    if s.is_empty() {
        return serde_json::Value::Null;
    }
    let unquoted = if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    };

    if unquoted.eq_ignore_ascii_case("true") {
        return serde_json::Value::Bool(true);
    }
    if unquoted.eq_ignore_ascii_case("false") {
        return serde_json::Value::Bool(false);
    }
    if let Ok(n) = unquoted.parse::<i64>() {
        return serde_json::json!(n);
    }
    if let Ok(n) = unquoted.parse::<f64>() {
        return serde_json::json!(n);
    }
    serde_json::Value::String(unquoted.to_string())
}

// ── IPC wire types ──────────────────────────────────────────────────────────

/// Args for `execute_database_view`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteDatabaseViewArgs {
    /// Forge-relative path of the `.bases` directory the inline block
    /// targets.
    pub database_path: String,
    /// View configuration carried by the
    /// [`crate::BlockType::DatabaseView`] block.
    pub view_config: DatabaseViewConfig,
}

/// Response from `execute_database_view`. Wraps `apply_view`'s
/// [`AppliedView`](nexus_database_views_applied) plus the
/// [`BaseSchema`] the renderer needs for column types and ordering.
///
/// `applied` is an opaque `serde_json::Value` so the editor crate
/// can stay independent of `nexus-database`'s `AppliedView`
/// definition — the wire shape is whatever
/// [`nexus_database::views::AppliedView`] serialises to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteDatabaseViewResponse {
    /// `apply_view` output (filtered + sorted + grouped).
    pub applied: serde_json::Value,
    /// Schema of the resolved base, so the renderer can format cells
    /// without a second IPC roundtrip.
    pub schema: BaseSchema,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_filter_symbolic_operators() {
        let f = parse_filter_string("status = Done").unwrap();
        assert_eq!(f.field, "status");
        assert_eq!(f.operator, "eq");
        assert_eq!(f.value, serde_json::json!("Done"));

        let f = parse_filter_string("priority >= 3").unwrap();
        assert_eq!(f.field, "priority");
        assert_eq!(f.operator, "gte");
        assert_eq!(f.value, serde_json::json!(3));

        let f = parse_filter_string("title != 'old name'").unwrap();
        assert_eq!(f.operator, "neq");
        assert_eq!(f.value, serde_json::json!("old name"));
    }

    #[test]
    fn parse_filter_word_operators() {
        let f = parse_filter_string("title contains foo").unwrap();
        assert_eq!(f.operator, "contains");
        assert_eq!(f.value, serde_json::json!("foo"));

        let f = parse_filter_string("body icontains \"Hello\"").unwrap();
        assert_eq!(f.operator, "icontains");
        assert_eq!(f.value, serde_json::json!("Hello"));

        // Field name containing the operator word (e.g. "contains_pii")
        // must not match the bare-word path.
        let f = parse_filter_string("contains_pii = true").unwrap();
        assert_eq!(f.field, "contains_pii");
        assert_eq!(f.operator, "eq");
        assert_eq!(f.value, serde_json::json!(true));
    }

    #[test]
    fn parse_filter_suffix_operators() {
        let f = parse_filter_string("title is_empty").unwrap();
        assert_eq!(f.field, "title");
        assert_eq!(f.operator, "is_empty");
        assert!(f.value.is_null());

        let f = parse_filter_string("body is_not_empty").unwrap();
        assert_eq!(f.operator, "is_not_empty");
    }

    #[test]
    fn parse_filter_rejects_garbage() {
        assert!(matches!(
            parse_filter_string("nothing here"),
            Err(TranslateError::UnrecognisedFilter(_))
        ));
        assert!(matches!(
            parse_filter_string("= 5"),
            Err(TranslateError::EmptyFilterField(_))
        ));
    }

    #[test]
    fn parse_sort_default_asc() {
        let s = parse_sort_string("due_date").unwrap();
        assert_eq!(s.field, "due_date");
        assert_eq!(s.direction, "asc");
    }

    #[test]
    fn parse_sort_explicit_desc() {
        let s = parse_sort_string("priority DESC").unwrap();
        assert_eq!(s.direction, "desc");
    }

    #[test]
    fn parse_sort_rejects_bad_direction() {
        assert!(matches!(
            parse_sort_string("priority sideways"),
            Err(TranslateError::InvalidSortDirection(_))
        ));
        assert!(matches!(
            parse_sort_string(""),
            Err(TranslateError::InvalidSort(_))
        ));
        assert!(matches!(
            parse_sort_string("a b c"),
            Err(TranslateError::InvalidSort(_))
        ));
    }

    #[test]
    fn config_to_view_table_roundtrip() {
        let config = DatabaseViewConfig {
            view_type: DatabaseViewType::Table,
            filters: vec!["status = Done".to_string(), "priority > 2".to_string()],
            sorts: vec!["due_date asc".to_string()],
            group_by: None,
            hidden_columns: Vec::new(),
        };
        let view = config_to_view("inline", &config).unwrap();
        assert_eq!(view.name, "inline");
        assert!(matches!(view.view_type, ViewType::Table));
        assert_eq!(view.filter.len(), 2);
        assert_eq!(view.sort.len(), 1);
        assert!(view.group_field.is_none());
        assert!(view.date_field.is_none());
    }

    #[test]
    fn config_to_view_kanban_uses_column_by() {
        let config = DatabaseViewConfig {
            view_type: DatabaseViewType::Kanban {
                column_by: "status".to_string(),
            },
            filters: Vec::new(),
            sorts: Vec::new(),
            group_by: Some("ignored_when_kanban_explicit".to_string()),
            hidden_columns: Vec::new(),
        };
        let view = config_to_view("", &config).unwrap();
        assert!(matches!(view.view_type, ViewType::Kanban));
        // Kanban's `column_by` wins over the generic `group_by`.
        assert_eq!(view.group_field.as_deref(), Some("status"));
    }

    #[test]
    fn config_to_view_calendar_uses_date_field() {
        let config = DatabaseViewConfig {
            view_type: DatabaseViewType::Calendar {
                date_field: "due".to_string(),
            },
            filters: Vec::new(),
            sorts: Vec::new(),
            group_by: None,
            hidden_columns: Vec::new(),
        };
        let view = config_to_view("", &config).unwrap();
        assert!(matches!(view.view_type, ViewType::Calendar));
        assert_eq!(view.date_field.as_deref(), Some("due"));
    }

    #[test]
    fn config_to_view_propagates_filter_errors() {
        let config = DatabaseViewConfig {
            view_type: DatabaseViewType::Table,
            filters: vec!["nonsense".to_string()],
            sorts: Vec::new(),
            group_by: None,
            hidden_columns: Vec::new(),
        };
        assert!(matches!(
            config_to_view("", &config),
            Err(TranslateError::UnrecognisedFilter(_))
        ));
    }
}
