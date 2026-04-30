//! BL-012 close-out — native parser / serializer for the inline
//! `[[{db:<path>?<query>}]]` syntax.
//!
//! Mirrors the JS-side parser in
//! `shell/src/plugins/nexus/editor/cm/databaseViewDecorations.ts`
//! so a `[[{db:…}]]` paragraph promotes to a
//! [`BlockType::DatabaseView`] at parse time and round-trips back
//! through the serializer with byte-identical output. Before this
//! lands, the editor block-tree treated the syntax as a regular
//! paragraph and the CM widget scanned source text directly — which
//! works for rendering but means the editor's undo tree only sees
//! free-text replacements rather than block-level edits to the
//! `view_config` field.
//!
//! Wire format (table view, no filters):
//!
//! ```text
//! [[{db:Tasks.bases}]]
//! ```
//!
//! With kanban + group + filter + sort + hide:
//!
//! ```text
//! [[{db:Tasks.bases?view=kanban&group=status&filter=status%20=%20Done&sort=due_date%20asc&hide=notes}]]
//! ```
//!
//! The query string is percent-encoded per `encodeURIComponent`
//! semantics (matches the JS side); `+` is decoded to space on the
//! way in (`URLSearchParams` compatibility).

use crate::block::{DatabaseViewConfig, DatabaseViewType};

/// Errors surfaced when the inline spec is malformed. Kept local
/// to this module — the parser callers degrade to "treat as a
/// paragraph" so a typo doesn't crash the document.
#[derive(Debug, PartialEq, Eq)]
pub enum DatabaseViewSpecError {
    /// The path before the `?` was empty.
    MissingPath,
    /// The path contained `..` (path-traversal guard, mirrors JS side).
    InvalidPath,
    /// `view=` carried a value that doesn't map to a known kind.
    UnknownView(String),
}

/// Parse the contents of a `[[{db:…}]]` block (the `…` payload — the
/// caller must have already stripped the `[[{db:` and `}]]` wrappers).
///
/// # Errors
/// Returns [`DatabaseViewSpecError`] for the cases listed in the
/// enum docs. Callers should fall back to treating the source as a
/// regular paragraph on error.
pub fn parse_database_view_spec(
    spec: &str,
) -> Result<(String, DatabaseViewConfig), DatabaseViewSpecError> {
    let trimmed = spec.trim();
    let (path_str, query_str) = match trimmed.find('?') {
        Some(i) => (trimmed[..i].trim(), Some(&trimmed[i + 1..])),
        None => (trimmed, None),
    };
    if path_str.is_empty() {
        return Err(DatabaseViewSpecError::MissingPath);
    }
    if path_str.contains("..") {
        return Err(DatabaseViewSpecError::InvalidPath);
    }
    let database_path = path_str.to_string();
    let mut config = DatabaseViewConfig::default();

    let Some(query) = query_str else {
        return Ok((database_path, config));
    };

    let mut view_kind: Option<String> = None;
    let mut group: Option<String> = None;
    let mut date_field: Option<String> = None;
    let mut title_field: Option<String> = None;

    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (raw_key, raw_value) = match pair.find('=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => (pair, ""),
        };
        let key = decode_form_value(raw_key);
        let value = decode_form_value(raw_value);
        match key.as_str() {
            "view" => view_kind = Some(value.to_lowercase()),
            "group" => group = Some(value),
            "date" | "date_field" => date_field = Some(value),
            "title" | "title_field" => title_field = Some(value),
            "filter" => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    config.filters.push(trimmed.to_string());
                }
            }
            "sort" => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    config.sorts.push(trimmed.to_string());
                }
            }
            "hide" => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    config.hidden_columns.push(trimmed.to_string());
                }
            }
            _ => {
                // Unknown query params are dropped silently to mirror
                // URLSearchParams' tolerance — a future param can't
                // be a parse-time error.
            }
        }
    }

    config.view_type = resolve_view_type(
        view_kind.as_deref(),
        group.as_deref(),
        date_field.as_deref(),
        title_field.as_deref(),
    )?;
    // Layout-specific fields (kanban `column_by`) win over the
    // generic `group_by` fallback, matching the JS-side rule.
    if let (Some(g), kind) = (group.as_deref(), &config.view_type) {
        if !matches!(kind, DatabaseViewType::Kanban { .. }) {
            config.group_by = Some(g.to_string());
        }
    }

    Ok((database_path, config))
}

/// Serialize a `(database_path, config)` pair back to the inline
/// `[[{db:<spec>}]]` form. Bare-table case round-trips to
/// `[[{db:<path>}]]` (no query string) so the source stays minimal.
#[must_use]
pub fn serialize_database_view_spec(database_path: &str, config: &DatabaseViewConfig) -> String {
    let mut params: Vec<String> = Vec::new();

    match &config.view_type {
        DatabaseViewType::Table => {
            // Default — omit `view=` to keep the source minimal.
        }
        DatabaseViewType::Kanban { column_by } => {
            params.push("view=kanban".to_string());
            params.push(format!("group={}", encode_form_value(column_by)));
        }
        DatabaseViewType::Calendar { date_field } => {
            params.push("view=calendar".to_string());
            params.push(format!("date={}", encode_form_value(date_field)));
        }
        DatabaseViewType::Gallery { title_field } => {
            params.push("view=gallery".to_string());
            params.push(format!("title={}", encode_form_value(title_field)));
        }
        DatabaseViewType::Custom(name) => {
            params.push(format!("view={}", encode_form_value(name)));
        }
    }

    if let Some(g) = &config.group_by {
        if !matches!(config.view_type, DatabaseViewType::Kanban { .. }) {
            params.push(format!("group={}", encode_form_value(g)));
        }
    }
    for f in &config.filters {
        params.push(format!("filter={}", encode_form_value(f)));
    }
    for s in &config.sorts {
        params.push(format!("sort={}", encode_form_value(s)));
    }
    for h in &config.hidden_columns {
        params.push(format!("hide={}", encode_form_value(h)));
    }

    if params.is_empty() {
        format!("[[{{db:{database_path}}}]]")
    } else {
        format!("[[{{db:{database_path}?{}}}]]", params.join("&"))
    }
}

/// Detect the `[[{db:…}]]` wrapper around a paragraph's content and
/// return the inner spec string. Mirrors `bare_embed_target`'s
/// shape so paragraph-promotion in `parse.rs` follows the same
/// pattern. Returns `None` when the content isn't exactly one
/// `[[{db:…}]]` (allowing surrounding whitespace).
#[must_use]
pub fn bare_database_view_target(content: &str) -> Option<&str> {
    let trimmed = content.trim();
    let rest = trimmed.strip_prefix("[[{db:")?.strip_suffix("}]]")?;
    // Reject nested `[[` / `]]` to keep the match unambiguous —
    // matches the `bare_embed_target` defence against lookalike
    // strings inside the body.
    if rest.contains("[[") || rest.contains("]]") {
        return None;
    }
    Some(rest)
}

fn resolve_view_type(
    view: Option<&str>,
    group: Option<&str>,
    date_field: Option<&str>,
    title_field: Option<&str>,
) -> Result<DatabaseViewType, DatabaseViewSpecError> {
    match view {
        None | Some("" | "table") => Ok(DatabaseViewType::Table),
        Some("kanban") => Ok(DatabaseViewType::Kanban {
            column_by: group.unwrap_or("status").to_string(),
        }),
        Some("calendar") => Ok(DatabaseViewType::Calendar {
            date_field: date_field.unwrap_or("date").to_string(),
        }),
        Some("gallery") => Ok(DatabaseViewType::Gallery {
            title_field: title_field.unwrap_or("title").to_string(),
        }),
        Some(other) => Err(DatabaseViewSpecError::UnknownView(other.to_string())),
    }
}

// ── Percent-encoding helpers ─────────────────────────────────────────────────
//
// The JS side uses `encodeURIComponent`, which preserves the URL
// "unreserved" set plus `!*'()`. URLSearchParams (the parser side)
// additionally decodes `+` to space for backward compatibility. We
// mirror both: encoder produces `encodeURIComponent`-equivalent
// output; decoder accepts `%XX` *and* `+`.

fn is_unreserved_for_form(byte: u8) -> bool {
    matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
                  | b'-' | b'_' | b'.' | b'~'
                  | b'!' | b'*' | b'\'' | b'(' | b')')
}

fn encode_form_value(value: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if is_unreserved_for_form(byte) {
            out.push(byte as char);
        } else {
            // Infallible — writing to String never errors.
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Decode a `%XX` / `+`-encoded form value. Invalid `%XX` sequences
/// pass through verbatim — the JS-side parser doesn't crash on them,
/// so we don't either.
fn decode_form_value(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = decode_hex(bytes[i + 1]);
                let lo = decode_hex(bytes[i + 2]);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h << 4) | l);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    // Lossless fallback to `from_utf8_lossy` — any malformed bytes
    // surface as the replacement char rather than panicking.
    String::from_utf8_lossy(&out).into_owned()
}

fn decode_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bare_path_yields_default_table_view() {
        let (path, config) = parse_database_view_spec("Tasks.bases").unwrap();
        assert_eq!(path, "Tasks.bases");
        assert_eq!(config, DatabaseViewConfig::default());
    }

    #[test]
    fn parse_kanban_with_group_populates_column_by() {
        let (path, config) =
            parse_database_view_spec("Tasks.bases?view=kanban&group=status").unwrap();
        assert_eq!(path, "Tasks.bases");
        assert_eq!(
            config.view_type,
            DatabaseViewType::Kanban {
                column_by: "status".to_string()
            }
        );
        // group= belongs to the layout-specific field, not group_by.
        assert!(config.group_by.is_none());
    }

    #[test]
    fn parse_filter_sort_hide_collected_in_order() {
        let (_, config) = parse_database_view_spec(
            "T.bases?filter=a%20=%201&filter=b%20!=%202&sort=due_date%20asc&hide=notes",
        )
        .unwrap();
        assert_eq!(config.filters, vec!["a = 1", "b != 2"]);
        assert_eq!(config.sorts, vec!["due_date asc"]);
        assert_eq!(config.hidden_columns, vec!["notes"]);
    }

    #[test]
    fn parse_calendar_uses_date_field_alias() {
        let (_, config) = parse_database_view_spec("E.bases?view=calendar&date=when").unwrap();
        assert_eq!(
            config.view_type,
            DatabaseViewType::Calendar {
                date_field: "when".to_string()
            }
        );
        // The `date_field=` alias is honoured too.
        let (_, config2) =
            parse_database_view_spec("E.bases?view=calendar&date_field=when").unwrap();
        assert_eq!(config2.view_type, config.view_type);
    }

    #[test]
    fn parse_rejects_empty_path() {
        assert_eq!(
            parse_database_view_spec(""),
            Err(DatabaseViewSpecError::MissingPath)
        );
        assert_eq!(
            parse_database_view_spec("?view=table"),
            Err(DatabaseViewSpecError::MissingPath)
        );
    }

    #[test]
    fn parse_rejects_path_traversal() {
        assert_eq!(
            parse_database_view_spec("../escape/x.bases"),
            Err(DatabaseViewSpecError::InvalidPath)
        );
    }

    #[test]
    fn parse_rejects_unknown_view_kind() {
        let err = parse_database_view_spec("T.bases?view=tarot").unwrap_err();
        assert_eq!(err, DatabaseViewSpecError::UnknownView("tarot".into()));
    }

    #[test]
    fn parse_decodes_plus_to_space_for_url_search_params_compat() {
        let (_, config) = parse_database_view_spec("T.bases?filter=name+is+set").unwrap();
        assert_eq!(config.filters, vec!["name is set"]);
    }

    #[test]
    fn serialize_table_omits_query_string() {
        let out = serialize_database_view_spec("Tasks.bases", &DatabaseViewConfig::default());
        assert_eq!(out, "[[{db:Tasks.bases}]]");
    }

    #[test]
    fn serialize_kanban_emits_view_and_group() {
        let config = DatabaseViewConfig {
            view_type: DatabaseViewType::Kanban {
                column_by: "status".to_string(),
            },
            ..Default::default()
        };
        let out = serialize_database_view_spec("Tasks.bases", &config);
        assert_eq!(out, "[[{db:Tasks.bases?view=kanban&group=status}]]");
    }

    #[test]
    fn serialize_filters_sorts_hides_percent_encode_special_chars() {
        let config = DatabaseViewConfig {
            filters: vec!["status = Done".to_string()],
            sorts: vec!["due_date asc".to_string()],
            hidden_columns: vec!["notes & memos".to_string()],
            ..Default::default()
        };
        let out = serialize_database_view_spec("T.bases", &config);
        // Spaces → %20; `&` → %26; `=` → %3D.
        assert!(out.contains("filter=status%20%3D%20Done"));
        assert!(out.contains("sort=due_date%20asc"));
        assert!(out.contains("hide=notes%20%26%20memos"));
    }

    #[test]
    fn round_trip_kanban_with_filters_is_byte_identical() {
        let config = DatabaseViewConfig {
            view_type: DatabaseViewType::Kanban {
                column_by: "status".to_string(),
            },
            filters: vec!["status = Done".to_string()],
            sorts: vec!["due_date desc".to_string()],
            ..Default::default()
        };
        let serialized = serialize_database_view_spec("Tasks.bases", &config);
        let inner = serialized
            .strip_prefix("[[{db:")
            .and_then(|s| s.strip_suffix("}]]"))
            .unwrap();
        let (path, parsed) = parse_database_view_spec(inner).unwrap();
        assert_eq!(path, "Tasks.bases");
        assert_eq!(parsed, config);
    }

    #[test]
    fn round_trip_table_with_group_by_keeps_group_distinct_from_kanban_column() {
        // group_by on a table view should persist through the
        // generic `group=` param (not collapsed into a kanban
        // column_by).
        let config = DatabaseViewConfig {
            group_by: Some("priority".to_string()),
            ..Default::default()
        };
        let serialized = serialize_database_view_spec("T.bases", &config);
        let inner = serialized
            .strip_prefix("[[{db:")
            .and_then(|s| s.strip_suffix("}]]"))
            .unwrap();
        let (_, parsed) = parse_database_view_spec(inner).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn bare_database_view_target_extracts_inner_spec() {
        assert_eq!(
            bare_database_view_target("[[{db:Tasks.bases}]]"),
            Some("Tasks.bases")
        );
        assert_eq!(
            bare_database_view_target("  [[{db:Tasks.bases?view=kanban}]]  "),
            Some("Tasks.bases?view=kanban")
        );
    }

    #[test]
    fn bare_database_view_target_rejects_lookalikes() {
        assert_eq!(bare_database_view_target("[[{db:T.bases}]] trailing"), None);
        assert_eq!(bare_database_view_target("[[T.bases]]"), None);
        assert_eq!(bare_database_view_target("[[{db:[[nested]]}]]"), None);
        assert_eq!(bare_database_view_target("![[{db:T.bases}]]"), None);
    }
}
