//! Notion exports often place a small 2-column table immediately after the
//! page's H1 heading to enumerate the page's database properties:
//!
//! ```markdown
//! # Page Title
//!
//! | Status | In Progress |
//! | ------ | ----------- |
//! | Owner  | Alex        |
//! | Due    | 2026-06-01  |
//!
//! Body starts here.
//! ```
//!
//! [`extract_property_table`] detects this pattern, returns a key-value map,
//! and the body with the table removed (and the H1 stripped — Nexus stores
//! the title in frontmatter or derives from filename).

use std::collections::BTreeMap;

/// Detect and extract a Notion property table.
///
/// Returns `(props, remaining_body)`:
///
/// - `props` is `Some(BTreeMap)` when a 2-column table was found
///   immediately after the H1.
/// - `remaining_body` is the input with the H1 and the property table
///   removed (or the original input if nothing matched).
#[must_use]
pub fn extract_property_table(input: &str) -> (Option<BTreeMap<String, String>>, String) {
    let mut lines = input.lines().enumerate().peekable();

    // 1. Skip leading blank lines.
    while matches!(lines.peek(), Some((_, l)) if l.trim().is_empty()) {
        lines.next();
    }

    // 2. Optional H1 — we drop it (title goes in frontmatter via filename).
    let _h1_consumed = matches!(lines.peek(), Some((_, l)) if l.starts_with("# "));
    if _h1_consumed {
        lines.next();
    }

    // 3. Skip blank lines between H1 and table.
    while matches!(lines.peek(), Some((_, l)) if l.trim().is_empty()) {
        lines.next();
    }

    // 4. Notion's "property table" is a 2-column markdown table where every
    //    row is a key/value pair. To satisfy CommonMark we also need a
    //    separator row right after the first row. We treat both the row
    //    before the separator and the rows after it as data (Notion's first
    //    row is property data, not a column header).
    let first_row_idx = match lines.peek() {
        Some((idx, l)) if is_table_row(l) && count_columns(l) == 2 => *idx,
        _ => return (None, input.to_string()),
    };
    let first_row_line = lines.next().unwrap().1.to_string();

    let separator_ok = matches!(
        lines.peek(),
        Some((_, l)) if is_separator_row(l) && count_columns(l) == 2
    );
    if !separator_ok {
        return (None, input.to_string());
    }
    lines.next(); // consume separator

    let mut props = BTreeMap::new();

    // First row IS data.
    {
        let cells = split_row(&first_row_line);
        let key = cells[0].trim().to_string();
        let value = cells[1].trim().to_string();
        if !key.is_empty() {
            props.insert(key, value);
        }
    }

    let mut last_consumed_idx = first_row_idx + 1; // separator
    while let Some((idx, l)) = lines.peek().copied() {
        if !is_table_row(l) || count_columns(l) != 2 {
            break;
        }
        let cells = split_row(l);
        let key = cells[0].trim().to_string();
        let value = cells[1].trim().to_string();
        if !key.is_empty() {
            props.insert(key, value);
        }
        lines.next();
        last_consumed_idx = idx;
    }

    // 6. Reconstruct remaining body — everything after the last consumed line.
    let remaining: String = input
        .lines()
        .enumerate()
        .filter(|(i, _)| *i > last_consumed_idx)
        .map(|(_, l)| l.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let trimmed = remaining.trim_start_matches('\n').to_string();
    let final_body = if input.ends_with('\n') && !trimmed.ends_with('\n') {
        format!("{trimmed}\n")
    } else {
        trimmed
    };

    (Some(props), final_body)
}

fn is_table_row(line: &str) -> bool {
    let l = line.trim();
    l.starts_with('|') && l.ends_with('|') && l.len() >= 3
}

fn is_separator_row(line: &str) -> bool {
    let l = line.trim();
    if !is_table_row(l) {
        return false;
    }
    split_row(l)
        .iter()
        .all(|c| c.trim().chars().all(|ch| matches!(ch, '-' | ':' | ' ')) && !c.trim().is_empty())
}

fn count_columns(line: &str) -> usize {
    split_row(line).len()
}

fn split_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = &trimmed[1..trimmed.len() - 1];
    inner.split('|').map(|c| c.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_property_table() {
        let input = "# Tasks\n\n| Status | In Progress |\n| --- | --- |\n| Owner | Alex |\n| Due | 2026-06-01 |\n\nBody.\n";
        let (props, body) = extract_property_table(input);
        let p = props.expect("expected props");
        assert_eq!(p.get("Status").unwrap(), "In Progress");
        assert_eq!(p.get("Owner").unwrap(), "Alex");
        assert_eq!(p.get("Due").unwrap(), "2026-06-01");
        assert_eq!(body.trim_start(), "Body.\n");
    }

    #[test]
    fn ignores_3_column_table() {
        let input = "# Page\n\n| A | B | C |\n| - | - | - |\n| 1 | 2 | 3 |\n\nMore.\n";
        let (props, body) = extract_property_table(input);
        assert!(props.is_none());
        assert_eq!(body, input);
    }

    #[test]
    fn ignores_when_no_h1_and_no_table() {
        let input = "Just a paragraph.\n";
        let (props, body) = extract_property_table(input);
        assert!(props.is_none());
        assert_eq!(body, input);
    }

    #[test]
    fn handles_table_without_h1() {
        let input = "| Status | Done |\n| --- | --- |\n| Owner | Alex |\n\nBody.\n";
        let (props, body) = extract_property_table(input);
        let p = props.expect("expected props");
        assert_eq!(p.get("Status").unwrap(), "Done");
        assert_eq!(body.trim_start(), "Body.\n");
    }

    #[test]
    fn empty_input_returns_none() {
        let (props, body) = extract_property_table("");
        assert!(props.is_none());
        assert_eq!(body, "");
    }

    #[test]
    fn preserves_body_after_table() {
        let input = "# T\n\n| K | V |\n| - | - |\n| a | 1 |\n\n## Section\n\n- bullet\n- bullet\n";
        let (props, body) = extract_property_table(input);
        assert!(props.is_some());
        assert!(body.contains("## Section"));
        assert!(body.contains("- bullet"));
    }
}
