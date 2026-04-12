use std::io::{IsTerminal, Write};

use comfy_table::{Cell, Table};

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

/// The serialisation format used for command output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
    Jsonl,
    Table,
}

impl OutputFormat {
    /// Parse an output format from a string slice.
    ///
    /// Unrecognised values fall back to [`OutputFormat::Text`].
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => OutputFormat::Json,
            "jsonl" => OutputFormat::Jsonl,
            "table" => OutputFormat::Table,
            _ => OutputFormat::Text,
        }
    }
}

// ---------------------------------------------------------------------------
// Color detection
// ---------------------------------------------------------------------------

/// Returns `true` when color output should be used.
///
/// Color is disabled when:
/// - `no_color_flag` is set, or
/// - the `NO_COLOR` environment variable is set, or
/// - stdout is not a TTY.
pub fn use_color(no_color_flag: bool) -> bool {
    if no_color_flag {
        return false;
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stdout().is_terminal()
}

// ---------------------------------------------------------------------------
// Print helpers
// ---------------------------------------------------------------------------

/// Print a success response, optionally including structured data.
pub fn print_success(format: OutputFormat, message: &str, data: &serde_json::Value) {
    match format {
        OutputFormat::Text => {
            println!("{message}");
        }
        OutputFormat::Json => {
            let envelope = serde_json::json!({
                "status": "success",
                "message": message,
                "data": data,
            });
            println!("{}", serde_json::to_string_pretty(&envelope).unwrap_or_default());
        }
        OutputFormat::Jsonl => {
            let envelope = serde_json::json!({
                "status": "success",
                "message": message,
                "data": data,
            });
            println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
            let _ = std::io::stdout().flush();
        }
        OutputFormat::Table => {
            // For a simple success message there are no tabular columns — fall
            // back to text.
            println!("{message}");
        }
    }
}

/// Print a list of items as a table, JSON array, or plain lines.
pub fn print_list(format: OutputFormat, headers: &[&str], rows: &[Vec<String>]) {
    match format {
        OutputFormat::Text => {
            // Simple aligned output without external deps.
            for row in rows {
                println!("{}", row.join("  "));
            }
        }
        OutputFormat::Json => {
            let records: Vec<serde_json::Value> = rows
                .iter()
                .map(|row| {
                    let map: serde_json::Map<String, serde_json::Value> = headers
                        .iter()
                        .zip(row.iter())
                        .map(|(h, v)| (h.to_string(), serde_json::Value::String(v.clone())))
                        .collect();
                    serde_json::Value::Object(map)
                })
                .collect();
            let envelope = serde_json::json!({
                "status": "success",
                "data": records,
            });
            println!("{}", serde_json::to_string_pretty(&envelope).unwrap_or_default());
        }
        OutputFormat::Jsonl => {
            for row in rows {
                let map: serde_json::Map<String, serde_json::Value> = headers
                    .iter()
                    .zip(row.iter())
                    .map(|(h, v)| (h.to_string(), serde_json::Value::String(v.clone())))
                    .collect();
                println!("{}", serde_json::to_string(&serde_json::Value::Object(map)).unwrap_or_default());
            }
            let _ = std::io::stdout().flush();
        }
        OutputFormat::Table => {
            let mut table = Table::new();
            table.set_header(headers.iter().map(|h| Cell::new(h)));
            for row in rows {
                table.add_row(row.iter().map(|v| Cell::new(v)));
            }
            println!("{table}");
        }
    }
}

/// Print a single JSON value in the requested format.
pub fn print_value(format: OutputFormat, data: &serde_json::Value) {
    match format {
        OutputFormat::Text => {
            // Pretty-print JSON as the generic text representation when there
            // is no richer domain-specific rendering available.
            println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
        }
        OutputFormat::Json => {
            let envelope = serde_json::json!({
                "status": "success",
                "data": data,
            });
            println!("{}", serde_json::to_string_pretty(&envelope).unwrap_or_default());
        }
        OutputFormat::Jsonl => {
            let envelope = serde_json::json!({
                "status": "success",
                "data": data,
            });
            println!("{}", serde_json::to_string(&envelope).unwrap_or_default());
            let _ = std::io::stdout().flush();
        }
        OutputFormat::Table => {
            // Flatten object keys into a two-column key/value table.
            let mut table = Table::new();
            table.set_header(["Key", "Value"]);
            match data {
                serde_json::Value::Object(map) => {
                    for (k, v) in map {
                        let display = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => serde_json::to_string(other).unwrap_or_default(),
                        };
                        table.add_row([k.as_str(), display.as_str()]);
                    }
                }
                other => {
                    table.add_row(["value", &serde_json::to_string(other).unwrap_or_default()]);
                }
            }
            println!("{table}");
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_from_str() {
        assert_eq!(OutputFormat::from_str("json"), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str("JSON"), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str("jsonl"), OutputFormat::Jsonl);
        assert_eq!(OutputFormat::from_str("JSONL"), OutputFormat::Jsonl);
        assert_eq!(OutputFormat::from_str("table"), OutputFormat::Table);
        assert_eq!(OutputFormat::from_str("TABLE"), OutputFormat::Table);
        assert_eq!(OutputFormat::from_str("text"), OutputFormat::Text);
        assert_eq!(OutputFormat::from_str("TEXT"), OutputFormat::Text);
        assert_eq!(OutputFormat::from_str("unknown"), OutputFormat::Text);
        assert_eq!(OutputFormat::from_str(""), OutputFormat::Text);
    }

    #[test]
    fn no_color_flag_disables_color() {
        // When the flag is set the function must return false regardless of
        // whether stdout is a TTY or the NO_COLOR env var is set.
        assert!(!use_color(true));
    }
}
