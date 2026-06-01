//! Notion-format writer (the inverse of [`super::import_notion_zip`]).
//!
//! Walks a forge subdirectory and writes a Notion-compatible folder tree:
//!
//! - Each `.md` file becomes `Title <uuid>.md`. The UUID comes from the
//!   `notion_id` frontmatter field if present (round-trip fidelity); otherwise
//!   a fresh UUIDv7 is generated.
//! - Wikilinks (`[[Title]]`, `[[Title|alias]]`) are rewritten to Notion mention
//!   links (`[Title](Title%20uuid.md)`).
//! - Callouts (`> [!note] body`) become emoji block-quotes (`> 💡 body`).
//! - `.bases` files are written out as CSV (data loss is possible — bases
//!   may carry types and views that don't survive the trip).
//! - Other files are copied verbatim into a sibling `attachments/` if they
//!   look like attachments, otherwise into the output root.
//!
//! Round-trip note: importing the produced folder back through
//! [`super::import_notion_zip`] preserves filenames and `notion_id`s when
//! both passes run on the same content.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Result of an export pass.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ExportReport {
    /// Markdown pages written (with `<uuid>` suffixes).
    pub pages_written: usize,
    /// Database CSVs written.
    pub databases_written: usize,
    /// Attachment files copied.
    pub attachments_copied: usize,
    /// Non-fatal warnings (links to missing pages, malformed bases, etc.).
    pub warnings: Vec<String>,
    /// Output paths created, relative to `dest`.
    pub written: Vec<PathBuf>,
}

/// Export `source` (a directory inside or equal to a forge root) to a
/// Notion-compatible folder at `dest`.
///
/// `dest` is created if missing. Existing files in `dest` are overwritten.
///
/// # Errors
/// Returns I/O errors on filesystem failures.
pub fn export_to_notion(source: &Path, dest: &Path) -> Result<ExportReport> {
    if !source.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("source is not a directory: {}", source.display()),
        )
        .into());
    }
    std::fs::create_dir_all(dest)?;

    // Pass 1 — index every .md to build title → uuid map for link rewriting.
    let index = build_export_index(source)?;

    // Pass 2 — convert and write.
    let mut report = ExportReport::default();
    walk_and_export(source, source, dest, &index, &mut report)?;

    Ok(report)
}

// ── Index ────────────────────────────────────────────────────────────────────

/// Title → (relative path with UUID suffix) for every page in the source.
struct ExportIndex {
    pages: HashMap<String, PathBuf>,
}

fn build_export_index(source: &Path) -> Result<ExportIndex> {
    let mut pages = HashMap::new();
    visit_md_files(source, source, &mut |rel, abs| {
        let stem = rel
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        let uuid = read_or_generate_uuid(abs);
        let parent = rel.parent().unwrap_or_else(|| Path::new(""));
        let target_filename = format!("{stem} {uuid}.md");
        let target_rel = parent.join(target_filename);
        pages.insert(stem, target_rel);
        Ok(())
    })?;
    Ok(ExportIndex { pages })
}

fn read_or_generate_uuid(path: &Path) -> String {
    if let Ok(body) = std::fs::read_to_string(path) {
        if let Some(uid) = extract_notion_id(&body) {
            return uid;
        }
    }
    new_notion_uuid()
}

/// Generate a 32-character lowercase hex string. Notion's UUIDs aren't
/// version-tagged; any 32 hex chars round-trip cleanly.
fn new_notion_uuid() -> String {
    let id = uuid::Uuid::now_v7();
    id.simple().to_string()
}

fn extract_notion_id(body: &str) -> Option<String> {
    let (fm, _) = parse_frontmatter(body)?;
    fm.get("notion_id").cloned()
}

// ── Walker ───────────────────────────────────────────────────────────────────

fn visit_md_files(
    root: &Path,
    dir: &Path,
    cb: &mut dyn FnMut(&Path, &Path) -> Result<()>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue; // skip dotfiles, especially .forge/
        }
        if path.is_dir() {
            visit_md_files(root, &path, cb)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            cb(&rel, &path)?;
        }
    }
    Ok(())
}

fn walk_and_export(
    root: &Path,
    dir: &Path,
    dest: &Path,
    index: &ExportIndex,
    report: &mut ExportReport,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path);

        if path.is_dir() {
            walk_and_export(root, &path, dest, index, report)?;
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "md" => export_markdown(&path, rel, dest, index, report)?,
            "bases" => export_bases(&path, rel, dest, report)?,
            _ => copy_attachment(&path, rel, dest, report)?,
        }
    }
    Ok(())
}

fn export_markdown(
    abs: &Path,
    rel: &Path,
    dest: &Path,
    index: &ExportIndex,
    report: &mut ExportReport,
) -> Result<()> {
    let body = std::fs::read_to_string(abs)?;
    let (fm, body_after_fm) = match parse_frontmatter(&body) {
        Some((fm, rest)) => (fm, rest.to_string()),
        None => (HashMap::new(), body.clone()),
    };

    // Resolve our own target filename.
    let stem = rel
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();
    let parent = rel.parent().unwrap_or_else(|| Path::new(""));
    let target_rel = index
        .pages
        .get(&stem)
        .cloned()
        .unwrap_or_else(|| parent.join(format!("{stem} {}.md", new_notion_uuid())));
    let target_abs = dest.join(&target_rel);
    if let Some(p) = target_abs.parent() {
        std::fs::create_dir_all(p)?;
    }

    // Build the page body: optional H1 + property table + converted body.
    let mut out = String::new();
    out.push_str(&format!("# {stem}\n\n"));

    // Property table from frontmatter (skip notion_id and source; those
    // are import-side metadata, not user-visible properties).
    let displayable: Vec<(&String, &String)> = fm
        .iter()
        .filter(|(k, _)| !matches!(k.as_str(), "notion_id" | "source"))
        .collect();
    if !displayable.is_empty() {
        out.push_str("|  |  |\n");
        out.push_str("| --- | --- |\n");
        for (k, v) in &displayable {
            out.push_str(&format!("| {k} | {v} |\n"));
        }
        out.push('\n');
    }

    // Convert callouts and rewrite wikilinks.
    let converted = convert_to_notion_markdown(&body_after_fm, index, report);
    out.push_str(&converted);

    std::fs::write(&target_abs, out)?;
    report.pages_written += 1;
    report.written.push(
        target_abs
            .strip_prefix(dest)
            .unwrap_or(&target_abs)
            .to_path_buf(),
    );
    Ok(())
}

fn export_bases(abs: &Path, rel: &Path, dest: &Path, report: &mut ExportReport) -> Result<()> {
    let body = std::fs::read_to_string(abs)?;
    match bases_to_csv(&body) {
        Ok(csv) => {
            let target_rel = rel.with_extension("csv");
            let target_abs = dest.join(&target_rel);
            if let Some(p) = target_abs.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&target_abs, csv)?;
            report.databases_written += 1;
            report.written.push(
                target_abs
                    .strip_prefix(dest)
                    .unwrap_or(&target_abs)
                    .to_path_buf(),
            );
        }
        Err(e) => {
            report
                .warnings
                .push(format!("bases→csv failed for {}: {e}", rel.display()));
            copy_attachment(abs, rel, dest, report)?;
        }
    }
    Ok(())
}

fn copy_attachment(abs: &Path, rel: &Path, dest: &Path, report: &mut ExportReport) -> Result<()> {
    let target_abs = dest.join(rel);
    if let Some(p) = target_abs.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::copy(abs, &target_abs)?;
    report.attachments_copied += 1;
    report.written.push(
        target_abs
            .strip_prefix(dest)
            .unwrap_or(&target_abs)
            .to_path_buf(),
    );
    Ok(())
}

// ── Markdown conversions ────────────────────────────────────────────────────

/// Map a Nexus callout type to the emoji Notion uses in its export.
fn emoji_for_callout(kind: &str) -> &'static str {
    match kind.to_ascii_lowercase().as_str() {
        "tip" => "💭",
        "warning" => "⚠️",
        "danger" => "🛑",
        "info" => "ℹ️",
        "quote" => "💬",
        // "note" and any unrecognised callout fall back to the bulb.
        _ => "💡",
    }
}

fn convert_to_notion_markdown(
    input: &str,
    index: &ExportIndex,
    report: &mut ExportReport,
) -> String {
    let after_callouts = convert_callouts_to_emoji(input);
    rewrite_wikilinks(&after_callouts, index, report)
}

fn convert_callouts_to_emoji(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut first = true;
    for line in input.lines() {
        if !first {
            out.push('\n');
        }
        first = false;

        if let Some(rest) = line.strip_prefix("> [!") {
            if let Some(close) = rest.find(']') {
                let kind = &rest[..close];
                let after = rest[close + 1..].trim_start();
                let emoji = emoji_for_callout(kind);
                if after.is_empty() {
                    out.push_str(&format!("> {emoji}"));
                } else {
                    out.push_str(&format!("> {emoji} {after}"));
                }
                continue;
            }
        }
        out.push_str(line);
    }
    if input.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn rewrite_wikilinks(input: &str, index: &ExportIndex, report: &mut ExportReport) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < input.len() {
        if i + 1 < input.len() && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some((target, alias, end)) = parse_wikilink(input, i) {
                let display = alias.unwrap_or(target);
                if let Some(target_path) = index.pages.get(target) {
                    let url = super::percent_encode(
                        target_path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(""),
                    );
                    out.push_str(&format!("[{display}]({url})"));
                    i = end;
                    continue;
                }
                report
                    .warnings
                    .push(format!("wikilink target not found in source: [[{target}]]"));
                // Pass through the original wikilink — Notion won't resolve it,
                // but the user can fix manually.
                out.push_str(&input[i..end]);
                i = end;
                continue;
            }
        }
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&input[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn parse_wikilink(s: &str, start: usize) -> Option<(&str, Option<&str>, usize)> {
    let bytes = s.as_bytes();
    debug_assert_eq!(&bytes[start..start + 2], b"[[");
    let inner_start = start + 2;
    let mut i = inner_start;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b']' {
            let inner = &s[inner_start..i];
            if inner.contains('\n') {
                return None;
            }
            let (target, alias) = match inner.find('|') {
                Some(p) => (&inner[..p], Some(&inner[p + 1..])),
                None => (inner, None),
            };
            return Some((target, alias, i + 2));
        }
        if bytes[i] == b'\n' {
            return None;
        }
        i += 1;
    }
    None
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0xC0 {
        // ASCII (< 0x80) and UTF-8 continuation bytes (0x80..=0xBF) both
        // count as one byte here.
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

// ── Frontmatter parsing ─────────────────────────────────────────────────────

fn parse_frontmatter(body: &str) -> Option<(HashMap<String, String>, &str)> {
    let stripped = body.strip_prefix("---\n")?;
    let end = stripped.find("\n---\n")?;
    let yaml = &stripped[..end];
    let rest = &stripped[end + 5..];
    let mut map = HashMap::new();
    for line in yaml.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_string();
            let mut v = v.trim().to_string();
            if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
                v = v[1..v.len() - 1].replace("\\\"", "\"");
            }
            map.insert(k, v);
        }
    }
    Some((map, rest))
}

// ── Bases → CSV ─────────────────────────────────────────────────────────────

/// Convert a `.bases` TOML body to a CSV string. Reads `[[fields]]` for the
/// header and `[[records]]` for the data rows. Records that are missing a
/// field render as an empty cell.
pub fn bases_to_csv(toml_body: &str) -> Result<String> {
    let parsed: toml::Value = toml::from_str(toml_body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("toml: {e}")))?;

    let fields = parsed
        .get("fields")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let headers: Vec<String> = fields
        .iter()
        .filter_map(|f| {
            f.get("id")
                .and_then(|v| v.as_str())
                .map(std::string::ToString::to_string)
        })
        .collect();

    let records = parsed
        .get("records")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record(&headers)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("csv: {e}")))?;

    for rec in &records {
        let row: Vec<String> = headers
            .iter()
            .map(|h| {
                rec.get(h)
                    .map(|v| match v {
                        toml::Value::String(s) => s.clone(),
                        toml::Value::Integer(i) => i.to_string(),
                        toml::Value::Float(f) => f.to_string(),
                        toml::Value::Boolean(b) => b.to_string(),
                        toml::Value::Datetime(d) => d.to_string(),
                        toml::Value::Array(_) | toml::Value::Table(_) => {
                            serde_json::to_string(v).unwrap_or_default()
                        }
                    })
                    .unwrap_or_default()
            })
            .collect();
        wtr.write_record(&row).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("csv: {e}"))
        })?;
    }

    let bytes = wtr
        .into_inner()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("csv: {e}")))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(p: &Path, body: &str) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, body).unwrap();
    }

    #[test]
    fn writes_pages_with_uuid_suffix() {
        let src = tempdir().unwrap();
        let dest = tempdir().unwrap();
        write(&src.path().join("Hello.md"), "Body of hello.\n");

        let report = export_to_notion(src.path(), dest.path()).unwrap();
        assert_eq!(report.pages_written, 1);

        let entries: Vec<_> = fs::read_dir(dest.path()).unwrap().collect();
        let name = entries[0]
            .as_ref()
            .unwrap()
            .file_name()
            .into_string()
            .unwrap();
        assert!(name.starts_with("Hello "), "got {name}");
        assert!(name.ends_with(".md"));
        // Should be 32-hex UUID between "Hello " and ".md".
        let uid = &name[6..name.len() - 3];
        assert_eq!(uid.len(), 32);
        assert!(uid.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn round_trips_existing_notion_id() {
        let src = tempdir().unwrap();
        let dest = tempdir().unwrap();
        write(
            &src.path().join("Hello.md"),
            "---\nnotion_id: aaaa1111aaaa1111aaaa1111aaaa1111\n---\n\nBody.\n",
        );

        export_to_notion(src.path(), dest.path()).unwrap();
        assert!(dest
            .path()
            .join("Hello aaaa1111aaaa1111aaaa1111aaaa1111.md")
            .exists());
    }

    #[test]
    fn rewrites_wikilinks_to_mention_links() {
        let src = tempdir().unwrap();
        let dest = tempdir().unwrap();
        write(
            &src.path().join("A.md"),
            "---\nnotion_id: aaaa1111aaaa1111aaaa1111aaaa1111\n---\n\nSee [[B]] please.\n",
        );
        write(
            &src.path().join("B.md"),
            "---\nnotion_id: bbbb2222bbbb2222bbbb2222bbbb2222\n---\n\nThe other.\n",
        );

        export_to_notion(src.path(), dest.path()).unwrap();
        let a =
            fs::read_to_string(dest.path().join("A aaaa1111aaaa1111aaaa1111aaaa1111.md")).unwrap();
        assert!(
            a.contains("[B](B%20bbbb2222bbbb2222bbbb2222bbbb2222.md)"),
            "got:\n{a}"
        );
    }

    #[test]
    fn rewrites_aliased_wikilinks() {
        let src = tempdir().unwrap();
        let dest = tempdir().unwrap();
        write(
            &src.path().join("A.md"),
            "---\nnotion_id: aaaa1111aaaa1111aaaa1111aaaa1111\n---\n\nSee [[B|the other]] note.\n",
        );
        write(
            &src.path().join("B.md"),
            "---\nnotion_id: bbbb2222bbbb2222bbbb2222bbbb2222\n---\n\nThe other.\n",
        );

        export_to_notion(src.path(), dest.path()).unwrap();
        let a =
            fs::read_to_string(dest.path().join("A aaaa1111aaaa1111aaaa1111aaaa1111.md")).unwrap();
        assert!(
            a.contains("[the other](B%20bbbb2222bbbb2222bbbb2222bbbb2222.md)"),
            "got:\n{a}"
        );
    }

    #[test]
    fn converts_callouts_to_emoji() {
        let src = tempdir().unwrap();
        let dest = tempdir().unwrap();
        write(
            &src.path().join("X.md"),
            "---\nnotion_id: aaaa1111aaaa1111aaaa1111aaaa1111\n---\n\n> [!note] Hello.\n\n> [!warning] Watch out.\n",
        );

        export_to_notion(src.path(), dest.path()).unwrap();
        let body =
            fs::read_to_string(dest.path().join("X aaaa1111aaaa1111aaaa1111aaaa1111.md")).unwrap();
        assert!(body.contains("> 💡 Hello."), "{body}");
        assert!(body.contains("> ⚠️ Watch out."), "{body}");
    }

    #[test]
    fn promotes_frontmatter_to_property_table() {
        let src = tempdir().unwrap();
        let dest = tempdir().unwrap();
        write(
            &src.path().join("X.md"),
            "---\nnotion_id: aaaa1111aaaa1111aaaa1111aaaa1111\nStatus: Active\nOwner: Alex\n---\n\nBody.\n",
        );

        export_to_notion(src.path(), dest.path()).unwrap();
        let body =
            fs::read_to_string(dest.path().join("X aaaa1111aaaa1111aaaa1111aaaa1111.md")).unwrap();
        assert!(body.contains("| Status | Active |"), "{body}");
        assert!(body.contains("| Owner | Alex |"), "{body}");
        assert!(
            !body.contains("notion_id"),
            "should not leak notion_id: {body}"
        );
    }

    #[test]
    fn bases_export_writes_csv() {
        let src = tempdir().unwrap();
        let dest = tempdir().unwrap();
        write(
            &src.path().join("Tasks.bases"),
            r#"name = "Tasks"

[[fields]]
id = "Name"
type = "string"

[[fields]]
id = "Score"
type = "number"

[[records]]
Name = "A"
Score = 1

[[records]]
Name = "B"
Score = 2
"#,
        );

        let report = export_to_notion(src.path(), dest.path()).unwrap();
        assert_eq!(report.databases_written, 1);
        let csv = fs::read_to_string(dest.path().join("Tasks.csv")).unwrap();
        assert!(csv.starts_with("Name,Score"), "{csv}");
        assert!(csv.contains("A,1"), "{csv}");
        assert!(csv.contains("B,2"), "{csv}");
    }

    #[test]
    fn copies_attachments() {
        let src = tempdir().unwrap();
        let dest = tempdir().unwrap();
        write(&src.path().join("attachments/img.png"), "<binary>");
        write(&src.path().join("Page.md"), "Body.\n");

        let report = export_to_notion(src.path(), dest.path()).unwrap();
        assert_eq!(report.attachments_copied, 1);
        assert!(dest.path().join("attachments/img.png").exists());
    }

    #[test]
    fn skips_dotfiles_and_dotforge() {
        let src = tempdir().unwrap();
        let dest = tempdir().unwrap();
        write(&src.path().join(".forge/index.db"), "ignore me");
        write(&src.path().join("Page.md"), "Body.\n");

        let report = export_to_notion(src.path(), dest.path()).unwrap();
        assert_eq!(report.pages_written, 1);
        assert_eq!(report.attachments_copied, 0);
        assert!(!dest.path().join(".forge").exists());
    }

    #[test]
    fn unknown_wikilink_warns_but_continues() {
        let src = tempdir().unwrap();
        let dest = tempdir().unwrap();
        write(&src.path().join("A.md"), "Refs [[Missing]] target.\n");

        let report = export_to_notion(src.path(), dest.path()).unwrap();
        assert!(
            report.warnings.iter().any(|w| w.contains("Missing")),
            "expected warning, got {:?}",
            report.warnings
        );
    }
}
