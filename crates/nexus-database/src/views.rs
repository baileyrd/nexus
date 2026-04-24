//! View application — PRD-10 §4 (Board / List / Calendar / Gallery).
//!
//! # Role
//!
//! Takes a [`BaseView`] definition plus a [`BaseRecord`] slice and
//! returns an [`AppliedView`] — records filtered, sorted, and (for
//! kanban / calendar) grouped according to the view's rules. Pure
//! logic; does not touch SQLite or any UI layer.
//!
//! # Microkernel fit
//!
//! Plain library. `com.nexus.database` exposes this through its
//! `apply_view` handler so UI plugins call
//! `ipc_call("com.nexus.database", "apply_view", …)` instead of
//! linking this crate. Matches the pattern the rest of `nexus-database`
//! uses (CSV import/export, formula eval).
//!
//! # What "view" means here
//!
//! - **Table / Gallery** → flat, filtered + sorted record list. Gallery
//!   is the same data; the UI layer decides card-vs-row rendering.
//! - **Kanban** → grouped by `group_field` (one group per distinct
//!   value). Records within a group respect the view's sort order.
//! - **Calendar** → grouped by `date_field`, bucketed into ISO date
//!   strings (`YYYY-MM-DD`). Records within a day respect sort order.
//!
//! # What this is NOT
//!
//! - A UI. Rendering lives in `shell/` / `nexus-tui`.
//! - A SQL layer. Filters and sorts operate on in-memory
//!   [`BaseRecord`]s; callers that want index-accelerated scans go
//!   through `com.nexus.storage`'s `base_query` instead.
//! - A rollup / lookup resolver. Those compute derived values across
//!   relations and belong to the SQL layer.

use std::collections::BTreeMap;

use nexus_types::bases::{BaseRecord, BaseSchema, BaseView, FilterRule, SortRule, ViewType};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Result of applying a [`BaseView`] to a record set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedView {
    /// Name of the originating view.
    pub view_name: String,
    /// Display type (drives the caller's rendering pick).
    pub view_type: ViewType,
    /// Ordered list of fields the view wants to show. Same as the
    /// input `BaseView::fields` — forwarded so the caller doesn't have
    /// to thread both objects.
    pub fields: Vec<String>,
    /// Shape of the resulting record layout. Flat for Table/Gallery;
    /// grouped for Kanban/Calendar.
    pub layout: ViewLayout,
}

/// Shape of the records after view application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewLayout {
    /// One ordered list (Table, Gallery, or Kanban/Calendar with no
    /// grouping field configured — degenerate but valid).
    Flat {
        /// Records in sort order.
        records: Vec<BaseRecord>,
    },
    /// Ordered sequence of groups; each group carries a key + its
    /// records in sort order. Used by Kanban and Calendar.
    Grouped {
        /// Groups in key order (alphabetical for Kanban, chronological
        /// for Calendar).
        groups: Vec<ViewGroup>,
    },
}

/// One group inside [`ViewLayout::Grouped`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewGroup {
    /// Stringified group key (group field value for Kanban, ISO date
    /// `YYYY-MM-DD` for Calendar, or `"(none)"` when the field is
    /// absent / null).
    pub key: String,
    /// Records inside this group, already sorted.
    pub records: Vec<BaseRecord>,
}

/// Sentinel key used when a record has no value for the grouping
/// field. Visible in the rendered UI so users can spot stragglers.
pub const MISSING_GROUP_KEY: &str = "(none)";

/// Apply `view` to `records`, returning the filtered + sorted +
/// grouped result. `schema` is currently unused but kept in the
/// signature so future type-aware filters (date range comparisons,
/// numeric coercion) can layer on without breaking callers.
#[must_use]
pub fn apply_view(records: &[BaseRecord], _schema: &BaseSchema, view: &BaseView) -> AppliedView {
    // 1. Filter
    let filtered: Vec<BaseRecord> = records
        .iter()
        .filter(|r| view.filter.iter().all(|rule| matches_filter(r, rule)))
        .cloned()
        .collect();

    // 2. Sort (stable; later rules break ties from earlier ones)
    let mut sorted = filtered;
    sort_records(&mut sorted, &view.sort);

    // 3. Group (if applicable)
    let layout = match view.view_type {
        ViewType::Kanban => {
            if let Some(field) = view.group_field.as_deref() {
                ViewLayout::Grouped {
                    groups: group_by_field(&sorted, field),
                }
            } else {
                ViewLayout::Flat { records: sorted }
            }
        }
        ViewType::Calendar => {
            if let Some(field) = view.date_field.as_deref() {
                ViewLayout::Grouped {
                    groups: group_by_date(&sorted, field),
                }
            } else {
                ViewLayout::Flat { records: sorted }
            }
        }
        ViewType::Table | ViewType::Gallery => ViewLayout::Flat { records: sorted },
        // List groups by `group_field` when set; Timeline produces
        // swimlanes keyed on `group_field` (shell pairs this with
        // `date_field` / `end_field` for bar spans). Both fall back
        // to a flat layout when the grouping field is absent.
        ViewType::List | ViewType::Timeline => {
            if let Some(field) = view.group_field.as_deref() {
                ViewLayout::Grouped {
                    groups: group_by_field(&sorted, field),
                }
            } else {
                ViewLayout::Flat { records: sorted }
            }
        }
    };

    AppliedView {
        view_name: view.name.clone(),
        view_type: view.view_type.clone(),
        fields: view.fields.clone(),
        layout,
    }
}

// ── Filters ─────────────────────────────────────────────────────────────────

/// Evaluate one filter rule against a record. Unknown operators
/// return `false` (record is filtered out) — callers should use
/// [`validate_filter_operator`] to catch typos at load time.
fn matches_filter(record: &BaseRecord, rule: &FilterRule) -> bool {
    let value = record.fields.get(&rule.field).unwrap_or(&Value::Null);
    match rule.operator.as_str() {
        "eq" | "=" => json_eq(value, &rule.value),
        "neq" | "!=" => !json_eq(value, &rule.value),
        "gt" | ">" => json_cmp(value, &rule.value).map(|o| o.is_gt()).unwrap_or(false),
        "gte" | ">=" => json_cmp(value, &rule.value).map(|o| o.is_ge()).unwrap_or(false),
        "lt" | "<" => json_cmp(value, &rule.value).map(|o| o.is_lt()).unwrap_or(false),
        "lte" | "<=" => json_cmp(value, &rule.value).map(|o| o.is_le()).unwrap_or(false),
        "contains" => str_contains(value, &rule.value, false),
        "icontains" => str_contains(value, &rule.value, true),
        "starts_with" => str_starts_with(value, &rule.value, false),
        "ends_with" => str_ends_with(value, &rule.value, false),
        "is_empty" => is_empty_value(value),
        "is_not_empty" => !is_empty_value(value),
        "in" => {
            if let Value::Array(items) = &rule.value {
                items.iter().any(|v| json_eq(value, v))
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Whether `op` is a recognised filter operator. Useful for
/// load-time validation so a UI can reject malformed `views.toml`
/// before it silently filters every record out.
#[must_use]
pub fn validate_filter_operator(op: &str) -> bool {
    matches!(
        op,
        "eq" | "="
            | "neq"
            | "!="
            | "gt"
            | ">"
            | "gte"
            | ">="
            | "lt"
            | "<"
            | "lte"
            | "<="
            | "contains"
            | "icontains"
            | "starts_with"
            | "ends_with"
            | "is_empty"
            | "is_not_empty"
            | "in"
    )
}

fn json_eq(a: &Value, b: &Value) -> bool {
    // Numbers compare by value across int/float representations.
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        return (x - y).abs() < f64::EPSILON;
    }
    a == b
}

fn json_cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        return x.partial_cmp(&y);
    }
    if let (Some(x), Some(y)) = (a.as_str(), b.as_str()) {
        return Some(x.cmp(y));
    }
    None
}

fn str_contains(value: &Value, needle: &Value, case_insensitive: bool) -> bool {
    let (Some(haystack), Some(needle)) = (value.as_str(), needle.as_str()) else {
        return false;
    };
    if case_insensitive {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    } else {
        haystack.contains(needle)
    }
}

fn str_starts_with(value: &Value, prefix: &Value, case_insensitive: bool) -> bool {
    let (Some(s), Some(p)) = (value.as_str(), prefix.as_str()) else {
        return false;
    };
    if case_insensitive {
        s.to_lowercase().starts_with(&p.to_lowercase())
    } else {
        s.starts_with(p)
    }
}

fn str_ends_with(value: &Value, suffix: &Value, case_insensitive: bool) -> bool {
    let (Some(s), Some(sfx)) = (value.as_str(), suffix.as_str()) else {
        return false;
    };
    if case_insensitive {
        s.to_lowercase().ends_with(&sfx.to_lowercase())
    } else {
        s.ends_with(sfx)
    }
}

fn is_empty_value(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

// ── Sort ────────────────────────────────────────────────────────────────────

fn sort_records(records: &mut [BaseRecord], rules: &[SortRule]) {
    if rules.is_empty() {
        return;
    }
    records.sort_by(|a, b| {
        for rule in rules {
            let va = a.fields.get(&rule.field).unwrap_or(&Value::Null);
            let vb = b.fields.get(&rule.field).unwrap_or(&Value::Null);
            let ord = compare_with_direction(va, vb, sort_is_desc(&rule.direction));
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
}

/// Null values sort *after* non-null ones regardless of direction —
/// blank cells always sink to the bottom of the column. The asc/desc
/// flag only flips the ordering between non-null values, so a
/// reverse-sort button doesn't suddenly float empty rows to the top.
fn compare_with_direction(a: &Value, b: &Value, desc: bool) -> std::cmp::Ordering {
    match (a.is_null(), b.is_null()) {
        (true, true) => return std::cmp::Ordering::Equal,
        (true, false) => return std::cmp::Ordering::Greater,
        (false, true) => return std::cmp::Ordering::Less,
        (false, false) => {}
    }
    let ord = compare_non_null(a, b);
    if desc {
        ord.reverse()
    } else {
        ord
    }
}

fn compare_non_null(a: &Value, b: &Value) -> std::cmp::Ordering {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        return x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
    }
    if let (Some(x), Some(y)) = (a.as_bool(), b.as_bool()) {
        return x.cmp(&y);
    }
    let sa = a.as_str().map(str::to_owned).unwrap_or_else(|| a.to_string());
    let sb = b.as_str().map(str::to_owned).unwrap_or_else(|| b.to_string());
    sa.cmp(&sb)
}

fn sort_is_desc(dir: &str) -> bool {
    matches!(
        dir,
        "desc" | "DESC" | "Desc" | "descending" | "Descending" | "DESCENDING"
    )
}

// ── Group ───────────────────────────────────────────────────────────────────

fn group_by_field(records: &[BaseRecord], field: &str) -> Vec<ViewGroup> {
    let mut buckets: BTreeMap<String, Vec<BaseRecord>> = BTreeMap::new();
    for r in records {
        let key = stringify_group_key(r.fields.get(field));
        buckets.entry(key).or_default().push(r.clone());
    }
    buckets
        .into_iter()
        .map(|(key, records)| ViewGroup { key, records })
        .collect()
}

fn group_by_date(records: &[BaseRecord], field: &str) -> Vec<ViewGroup> {
    let mut buckets: BTreeMap<String, Vec<BaseRecord>> = BTreeMap::new();
    for r in records {
        let key = match r.fields.get(field) {
            Some(Value::String(s)) => iso_date_prefix(s),
            _ => MISSING_GROUP_KEY.to_string(),
        };
        buckets.entry(key).or_default().push(r.clone());
    }
    buckets
        .into_iter()
        .map(|(key, records)| ViewGroup { key, records })
        .collect()
}

/// Extract the `YYYY-MM-DD` prefix from a date/datetime string.
/// Returns `MISSING_GROUP_KEY` if the string is not at least 10 chars.
fn iso_date_prefix(s: &str) -> String {
    if s.len() >= 10 && s.chars().take(10).all(|c| c.is_ascii_digit() || c == '-') {
        s[..10].to_string()
    } else {
        MISSING_GROUP_KEY.to_string()
    }
}

fn stringify_group_key(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => MISSING_GROUP_KEY.to_string(),
        Some(Value::String(s)) if s.is_empty() => MISSING_GROUP_KEY.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Array(arr)) => {
            // Multi-select: join the values so each combination is its
            // own column. Stable by iteration order.
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
        Some(other) => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: &str, fields: serde_json::Value) -> BaseRecord {
        let map = fields.as_object().cloned().unwrap_or_default();
        BaseRecord {
            id: id.to_string(),
            deleted_at: None,
            fields: map,
        }
    }

    fn empty_schema() -> BaseSchema {
        BaseSchema {
            version: "1.0".into(),
            fields: serde_json::Map::new(),
        }
    }

    fn table_view(name: &str) -> BaseView {
        BaseView {
            name: name.into(),
            view_type: ViewType::Table,
            fields: vec!["title".into()],
            sort: vec![],
            filter: vec![],
            group_field: None,
            date_field: None,
            end_field: None,
        }
    }

    #[test]
    fn table_view_with_no_filters_or_sort_preserves_input_order() {
        let records = vec![
            record("a", json!({"title": "Alpha"})),
            record("b", json!({"title": "Beta"})),
        ];
        let view = table_view("All");
        let out = apply_view(&records, &empty_schema(), &view);
        assert_eq!(out.view_type, ViewType::Table);
        match out.layout {
            ViewLayout::Flat { records } => {
                assert_eq!(records.len(), 2);
                assert_eq!(records[0].id, "a");
                assert_eq!(records[1].id, "b");
            }
            other => panic!("expected Flat, got {other:?}"),
        }
    }

    #[test]
    fn filter_eq_drops_non_matches() {
        let records = vec![
            record("a", json!({"status": "todo"})),
            record("b", json!({"status": "done"})),
            record("c", json!({"status": "todo"})),
        ];
        let mut view = table_view("Todo");
        view.filter = vec![FilterRule {
            field: "status".into(),
            operator: "eq".into(),
            value: json!("todo"),
        }];
        let out = apply_view(&records, &empty_schema(), &view);
        let ViewLayout::Flat { records } = out.layout else {
            panic!("expected Flat");
        };
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.fields["status"] == "todo"));
    }

    #[test]
    fn filter_numeric_comparisons() {
        let records = vec![
            record("a", json!({"priority": 1})),
            record("b", json!({"priority": 3})),
            record("c", json!({"priority": 5})),
        ];
        let mut view = table_view("High priority");
        view.filter = vec![FilterRule {
            field: "priority".into(),
            operator: "gte".into(),
            value: json!(3),
        }];
        let ViewLayout::Flat { records } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.fields["priority"].as_f64().unwrap() >= 3.0));
    }

    #[test]
    fn filter_string_operators() {
        let records = vec![
            record("a", json!({"name": "Alpha apple"})),
            record("b", json!({"name": "Beta banana"})),
            record("c", json!({"name": "Gamma"})),
        ];
        let mut view = table_view("A-fruits");
        view.filter = vec![FilterRule {
            field: "name".into(),
            operator: "contains".into(),
            value: json!("apple"),
        }];
        let ViewLayout::Flat { records: r1 } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].id, "a");

        view.filter = vec![FilterRule {
            field: "name".into(),
            operator: "starts_with".into(),
            value: json!("Beta"),
        }];
        let ViewLayout::Flat { records: r2 } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].id, "b");
    }

    #[test]
    fn filter_in_matches_any_in_list() {
        let records = vec![
            record("a", json!({"status": "todo"})),
            record("b", json!({"status": "wip"})),
            record("c", json!({"status": "done"})),
        ];
        let mut view = table_view("Active");
        view.filter = vec![FilterRule {
            field: "status".into(),
            operator: "in".into(),
            value: json!(["todo", "wip"]),
        }];
        let ViewLayout::Flat { records } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn filter_is_empty_vs_is_not_empty() {
        let records = vec![
            record("a", json!({"note": ""})),
            record("b", json!({"note": "hello"})),
            record("c", json!({})),
        ];
        let mut view = table_view("Missing notes");
        view.filter = vec![FilterRule {
            field: "note".into(),
            operator: "is_empty".into(),
            value: Value::Null,
        }];
        let ViewLayout::Flat { records: empty } =
            apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        assert_eq!(empty.len(), 2);

        view.filter = vec![FilterRule {
            field: "note".into(),
            operator: "is_not_empty".into(),
            value: Value::Null,
        }];
        let ViewLayout::Flat { records: present } =
            apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        assert_eq!(present.len(), 1);
        assert_eq!(present[0].id, "b");
    }

    #[test]
    fn sort_asc_and_desc_on_numeric_field() {
        let records = vec![
            record("a", json!({"priority": 3})),
            record("b", json!({"priority": 1})),
            record("c", json!({"priority": 5})),
        ];
        let mut view = table_view("Sorted asc");
        view.sort = vec![SortRule {
            field: "priority".into(),
            direction: "asc".into(),
        }];
        let ViewLayout::Flat { records: asc } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        assert_eq!(asc.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["b", "a", "c"]);

        view.sort = vec![SortRule {
            field: "priority".into(),
            direction: "desc".into(),
        }];
        let ViewLayout::Flat { records: desc } =
            apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        assert_eq!(desc.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["c", "a", "b"]);
    }

    #[test]
    fn multi_level_sort_breaks_ties_with_later_rules() {
        let records = vec![
            record("a", json!({"status": "todo", "priority": 1})),
            record("b", json!({"status": "todo", "priority": 3})),
            record("c", json!({"status": "done", "priority": 2})),
        ];
        let mut view = table_view("Sorted");
        view.sort = vec![
            SortRule { field: "status".into(), direction: "asc".into() },
            SortRule { field: "priority".into(), direction: "desc".into() },
        ];
        let ViewLayout::Flat { records } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        // `done` sorts before `todo`; within `todo`, priority 3 > 1
        // under desc.
        assert_eq!(
            records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["c", "b", "a"],
        );
    }

    #[test]
    fn sort_places_nulls_after_non_nulls_in_both_directions() {
        let records = vec![
            record("a", json!({"priority": 2})),
            record("b", json!({})),
            record("c", json!({"priority": 1})),
        ];
        let mut view = table_view("With nulls");
        view.sort = vec![SortRule {
            field: "priority".into(),
            direction: "asc".into(),
        }];
        let ViewLayout::Flat { records: asc } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        let ids: Vec<_> = asc.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a", "b"], "nulls last on asc");

        view.sort = vec![SortRule {
            field: "priority".into(),
            direction: "desc".into(),
        }];
        let ViewLayout::Flat { records: desc } =
            apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        let ids: Vec<_> = desc.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c", "b"], "nulls still last on desc");
    }

    #[test]
    fn kanban_groups_by_group_field() {
        let records = vec![
            record("a", json!({"status": "todo", "priority": 1})),
            record("b", json!({"status": "done", "priority": 2})),
            record("c", json!({"status": "todo", "priority": 3})),
        ];
        let view = BaseView {
            name: "Board".into(),
            view_type: ViewType::Kanban,
            fields: vec!["title".into()],
            sort: vec![SortRule {
                field: "priority".into(),
                direction: "asc".into(),
            }],
            filter: vec![],
            group_field: Some("status".into()),
            date_field: None,
            end_field: None,
        };
        let out = apply_view(&records, &empty_schema(), &view);
        let ViewLayout::Grouped { groups } = out.layout else {
            panic!("expected Grouped");
        };
        // BTreeMap ordering: `done` before `todo`.
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "done");
        assert_eq!(groups[0].records.len(), 1);
        assert_eq!(groups[1].key, "todo");
        assert_eq!(
            groups[1].records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "c"],
            "records within group stay in sort order",
        );
    }

    #[test]
    fn kanban_without_group_field_degrades_to_flat_layout() {
        let records = vec![record("a", json!({"status": "todo"}))];
        let view = BaseView {
            name: "Board".into(),
            view_type: ViewType::Kanban,
            fields: vec!["title".into()],
            sort: vec![],
            filter: vec![],
            group_field: None,
            date_field: None,
            end_field: None,
        };
        assert!(matches!(
            apply_view(&records, &empty_schema(), &view).layout,
            ViewLayout::Flat { .. },
        ));
    }

    #[test]
    fn kanban_null_group_key_bucketed_under_none_sentinel() {
        let records = vec![
            record("a", json!({"status": "todo"})),
            record("b", json!({})),
            record("c", json!({"status": ""})),
        ];
        let view = BaseView {
            name: "Board".into(),
            view_type: ViewType::Kanban,
            fields: vec!["title".into()],
            sort: vec![],
            filter: vec![],
            group_field: Some("status".into()),
            date_field: None,
            end_field: None,
        };
        let ViewLayout::Grouped { groups } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        let none_group = groups.iter().find(|g| g.key == MISSING_GROUP_KEY).unwrap();
        assert_eq!(none_group.records.len(), 2);
    }

    #[test]
    fn calendar_buckets_by_iso_date_prefix() {
        let records = vec![
            record("a", json!({"due": "2026-04-17T09:30:00Z"})),
            record("b", json!({"due": "2026-04-17T14:00:00Z"})),
            record("c", json!({"due": "2026-04-18T08:00:00Z"})),
            record("d", json!({"due": "nonsense"})),
        ];
        let view = BaseView {
            name: "Calendar".into(),
            view_type: ViewType::Calendar,
            fields: vec!["title".into()],
            sort: vec![],
            filter: vec![],
            group_field: None,
            date_field: Some("due".into()),
            end_field: None,
        };
        let ViewLayout::Grouped { groups } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        assert_eq!(groups.len(), 3); // 17th, 18th, (none)
        let apr17 = groups.iter().find(|g| g.key == "2026-04-17").unwrap();
        assert_eq!(apr17.records.len(), 2);
        let apr18 = groups.iter().find(|g| g.key == "2026-04-18").unwrap();
        assert_eq!(apr18.records.len(), 1);
        let none = groups.iter().find(|g| g.key == MISSING_GROUP_KEY).unwrap();
        assert_eq!(none.records.len(), 1);
    }

    #[test]
    fn gallery_is_flat_layout_like_table() {
        let records = vec![
            record("a", json!({"title": "A"})),
            record("b", json!({"title": "B"})),
        ];
        let view = BaseView {
            name: "Cards".into(),
            view_type: ViewType::Gallery,
            fields: vec!["title".into()],
            sort: vec![],
            filter: vec![],
            group_field: None,
            date_field: None,
            end_field: None,
        };
        let out = apply_view(&records, &empty_schema(), &view);
        assert_eq!(out.view_type, ViewType::Gallery);
        assert!(matches!(out.layout, ViewLayout::Flat { .. }));
    }

    #[test]
    fn validate_filter_operator_allows_known_and_rejects_unknown() {
        assert!(validate_filter_operator("eq"));
        assert!(validate_filter_operator(">="));
        assert!(validate_filter_operator("is_empty"));
        assert!(validate_filter_operator("in"));
        assert!(!validate_filter_operator("whatever"));
        assert!(!validate_filter_operator(""));
    }

    #[test]
    fn unknown_filter_operator_drops_all_records() {
        let records = vec![record("a", json!({"status": "todo"}))];
        let mut view = table_view("Broken");
        view.filter = vec![FilterRule {
            field: "status".into(),
            operator: "nonexistent".into(),
            value: json!("todo"),
        }];
        let ViewLayout::Flat { records } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        assert!(records.is_empty());
    }

    #[test]
    fn combined_filter_sort_group_pipeline_runs_in_order() {
        let records = vec![
            record("a", json!({"status": "todo", "priority": 3, "archived": false})),
            record("b", json!({"status": "todo", "priority": 1, "archived": false})),
            record("c", json!({"status": "done", "priority": 2, "archived": false})),
            record("d", json!({"status": "todo", "priority": 5, "archived": true})),
        ];
        let view = BaseView {
            name: "Active board".into(),
            view_type: ViewType::Kanban,
            fields: vec!["title".into()],
            sort: vec![SortRule {
                field: "priority".into(),
                direction: "desc".into(),
            }],
            filter: vec![FilterRule {
                field: "archived".into(),
                operator: "eq".into(),
                value: json!(false),
            }],
            group_field: Some("status".into()),
            date_field: None,
            end_field: None,
        };
        let ViewLayout::Grouped { groups } = apply_view(&records, &empty_schema(), &view).layout
        else {
            panic!();
        };
        // `d` filtered out (archived=true); then sorted desc by priority;
        // then grouped by status. BTreeMap: done < todo.
        assert_eq!(groups.len(), 2);
        let done = groups.iter().find(|g| g.key == "done").unwrap();
        let todo = groups.iter().find(|g| g.key == "todo").unwrap();
        assert_eq!(done.records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["c"]);
        assert_eq!(
            todo.records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"],
            "priority 3 > 1 under desc",
        );
    }
}
