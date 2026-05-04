//! CSV → `.bases` (TOML) conversion for Notion-exported databases.
//!
//! Notion exports each database as a CSV. We:
//!
//! 1. Read the header row to learn column names.
//! 2. Sample up to N body rows to *infer* a column type per column.
//! 3. Emit a `.bases` TOML file with `[[fields]]` blocks and a single
//!    default `[[views]]` of type `table`.
//!
//! Type inference cascade per column: `bool` → `number` → `date` →
//! `string`. The first cell that breaks a candidate disqualifies it.
//! Empty cells don't disqualify anything.

const SAMPLE_LIMIT: usize = 256;

/// Convert a CSV string to a `.bases` TOML body. `name` is the database
/// name (used as the `name = "…"` field at the top).
///
/// # Errors
/// Returns `Error::Io` if the CSV is malformed (wraps the csv crate's
/// error inside `io::Error::InvalidData`).
pub fn csv_to_bases(csv_str: &str, name: &str) -> crate::Result<String> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(csv_str.as_bytes());

    let headers: Vec<String> = rdr
        .headers()
        .map_err(io_err)?
        .iter()
        .map(|s| s.to_string())
        .collect();

    if headers.is_empty() {
        return Ok(format!(
            "name = \"{}\"\nrecords = \"\"\n\n[[views]]\nid = \"all\"\ntype = \"table\"\n",
            escape_toml_str(name)
        ));
    }

    // Collect a sample for type inference.
    let mut samples: Vec<Vec<String>> = Vec::with_capacity(SAMPLE_LIMIT);
    for (i, rec) in rdr.records().enumerate() {
        if i >= SAMPLE_LIMIT {
            break;
        }
        let rec = rec.map_err(io_err)?;
        samples.push(rec.iter().map(|s| s.to_string()).collect());
    }

    // Infer one type per column from the sample.
    let types: Vec<&'static str> = (0..headers.len())
        .map(|col_idx| infer_column_type(&samples, col_idx))
        .collect();

    // Emit TOML. Records are inlined as `[[records]]` array-of-tables; no
    // sentinel needed (the presence of `[[records]]` is the signal).
    let mut out = String::new();
    out.push_str(&format!("name = \"{}\"\n\n", escape_toml_str(name)));

    for (header, ty) in headers.iter().zip(types.iter()) {
        out.push_str("[[fields]]\n");
        out.push_str(&format!("id = \"{}\"\n", escape_toml_str(header)));
        out.push_str(&format!("type = \"{ty}\"\n"));
        out.push('\n');
    }

    out.push_str("[[views]]\n");
    out.push_str("id = \"all\"\n");
    out.push_str("type = \"table\"\n\n");

    // Inline records — render each row as a [[records]] block of typed
    // fields. Keeps the .bases file self-contained without a sibling JSON.
    for row in &samples {
        out.push_str("[[records]]\n");
        for (i, header) in headers.iter().enumerate() {
            let cell = row.get(i).map(String::as_str).unwrap_or("");
            if cell.is_empty() {
                continue;
            }
            let key = escape_toml_key(header);
            let value = format_typed_value(cell, types[i]);
            out.push_str(&format!("{key} = {value}\n"));
        }
        out.push('\n');
    }

    Ok(out)
}

fn io_err(e: csv::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, format!("csv: {e}"))
}

// ── Type inference ──────────────────────────────────────────────────────────

fn infer_column_type(samples: &[Vec<String>], col: usize) -> &'static str {
    let mut all_bool = true;
    let mut all_number = true;
    let mut all_date = true;
    let mut any_value = false;

    for row in samples {
        let cell = match row.get(col) {
            Some(s) if !s.trim().is_empty() => s.trim(),
            _ => continue,
        };
        any_value = true;
        if all_bool && !is_bool(cell) {
            all_bool = false;
        }
        if all_number && cell.parse::<f64>().is_err() {
            all_number = false;
        }
        if all_date && !is_iso_date(cell) {
            all_date = false;
        }
        if !all_bool && !all_number && !all_date {
            break;
        }
    }

    if !any_value {
        return "string";
    }
    if all_bool {
        "boolean"
    } else if all_number {
        "number"
    } else if all_date {
        "date"
    } else {
        "string"
    }
}

fn is_bool(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "true" | "false" | "yes" | "no" | "1" | "0"
    )
}

/// Recognize `YYYY-MM-DD`, optionally followed by `T…` time. Permissive
/// because Notion exports vary in date format slightly per locale.
fn is_iso_date(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return false;
    }
    bytes[..10]
        .iter()
        .enumerate()
        .all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
}

// ── TOML escaping ───────────────────────────────────────────────────────────

fn escape_toml_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Bare keys in TOML can be unquoted if they match `[A-Za-z0-9_-]+`.
/// Otherwise quote.
fn escape_toml_key(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        s.to_string()
    } else {
        format!("\"{}\"", escape_toml_str(s))
    }
}

fn format_typed_value(cell: &str, ty: &str) -> String {
    match ty {
        "boolean" => match cell.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => "true".to_string(),
            _ => "false".to_string(),
        },
        "number" => cell.to_string(),
        "date" | "string" => format!("\"{}\"", escape_toml_str(cell)),
        _ => format!("\"{}\"", escape_toml_str(cell)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_string_default() {
        let csv = "Name\nAlpha\nBeta\n";
        let out = csv_to_bases(csv, "x").unwrap();
        assert!(out.contains("type = \"string\""), "{out}");
    }

    #[test]
    fn infers_numeric_column() {
        let csv = "Name,Score\nAlpha,1.5\nBeta,2.0\n";
        let out = csv_to_bases(csv, "x").unwrap();
        assert!(out.contains("id = \"Score\"\ntype = \"number\""), "{out}");
    }

    #[test]
    fn infers_boolean_column() {
        let csv = "Name,Done\nAlpha,true\nBeta,false\n";
        let out = csv_to_bases(csv, "x").unwrap();
        assert!(out.contains("id = \"Done\"\ntype = \"boolean\""), "{out}");
    }

    #[test]
    fn infers_date_column() {
        let csv = "Name,Due\nAlpha,2026-06-01\nBeta,2026-07-15\n";
        let out = csv_to_bases(csv, "x").unwrap();
        assert!(out.contains("id = \"Due\"\ntype = \"date\""), "{out}");
    }

    #[test]
    fn mixed_column_falls_back_to_string() {
        let csv = "Name,Mix\nAlpha,1\nBeta,hello\n";
        let out = csv_to_bases(csv, "x").unwrap();
        assert!(out.contains("id = \"Mix\"\ntype = \"string\""), "{out}");
    }

    #[test]
    fn empty_cells_dont_disqualify() {
        let csv = "Name,Score\nAlpha,1\nBeta,\nGamma,3\n";
        let out = csv_to_bases(csv, "x").unwrap();
        assert!(out.contains("id = \"Score\"\ntype = \"number\""), "{out}");
    }

    #[test]
    fn includes_records_block() {
        let csv = "Name,Score\nAlpha,1\nBeta,2\n";
        let out = csv_to_bases(csv, "x").unwrap();
        assert!(out.contains("[[records]]"), "{out}");
        assert!(out.contains("Name = \"Alpha\""), "{out}");
        assert!(out.contains("Score = 1"), "{out}");
    }

    #[test]
    fn quotes_keys_with_special_chars() {
        let csv = "Has Space,Score\nA,1\nB,2\n";
        let out = csv_to_bases(csv, "x").unwrap();
        assert!(out.contains("\"Has Space\" = \"A\""), "{out}");
    }

    #[test]
    fn includes_default_table_view() {
        let csv = "A\nx\n";
        let out = csv_to_bases(csv, "x").unwrap();
        assert!(out.contains("[[views]]"), "{out}");
        assert!(out.contains("type = \"table\""), "{out}");
    }
}
