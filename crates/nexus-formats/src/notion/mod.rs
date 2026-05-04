//! Notion markdown-export importer (PRD-06 §10.2).
//!
//! Reads a Notion "Export → Markdown & CSV" zip and converts it into Nexus
//! markdown + bases. Conversion rules (see sub-modules for detail):
//!
//! - **Filenames**: `Page Name a1b2…32hex.md` → `Page Name.md`; the UUID is
//!   preserved as `notion_id` in YAML frontmatter for round-trip fidelity.
//! - **Folders**: nested page folders renamed the same way; cross-page
//!   references rewritten to the new paths.
//! - **Mention links**: `[Title](Title%20abc.md)` → `[[Title]]`.
//! - **Callouts**: leading-emoji block quotes (`> 💡 …`) → Nexus callouts
//!   (`> [!note] …`).
//! - **Property tables**: 2-column table immediately after the H1 → YAML
//!   frontmatter.
//! - **Databases**: each `*.csv` → a sibling `.bases` file with column types
//!   inferred from the data.
//! - **Attachments**: image/file refs preserved; assets copied verbatim.
//!
//! Unsupported blocks (synced blocks, formulas, advanced embeds) degrade
//! gracefully — they pass through as plain markdown and a warning is
//! recorded in the [`ImportReport`].

pub mod database;
pub mod export;
pub mod filename;
pub mod markdown;
pub mod property;

pub use export::{export_to_notion, ExportReport};

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Summary of an import run.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ImportReport {
    /// Markdown pages written.
    pub pages_written: usize,
    /// Bases files written (from CSV databases).
    pub bases_written: usize,
    /// Attachment files copied.
    pub attachments_copied: usize,
    /// Non-fatal warnings encountered (unknown blocks, malformed property
    /// tables, etc.). One entry per source path.
    pub warnings: Vec<String>,
    /// Output paths created, relative to `dest`.
    pub written: Vec<PathBuf>,
}

/// Import a Notion markdown-export zip into the destination directory.
///
/// `dest` is the target folder (typically a subdirectory of a forge). It
/// will be created if missing. Existing files are **not** overwritten;
/// collisions append a numeric suffix.
///
/// # Errors
/// Returns `Error::Io` on filesystem failures and a wrapped zip error if
/// the archive is malformed.
pub fn import_notion_zip(zip_path: &Path, dest: &Path) -> Result<ImportReport> {
    let file = std::fs::File::open(zip_path)?;
    import_notion_archive(file, dest)
}

/// Import a Notion zip from any [`Read`] + [`Seek`] source. Lets callers
/// import from in-memory buffers (useful for tests and IPC payloads).
///
/// # Errors
/// See [`import_notion_zip`].
pub fn import_notion_archive<R: Read + Seek>(
    reader: R,
    dest: &Path,
) -> Result<ImportReport> {
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("zip: {e}"))
    })?;

    std::fs::create_dir_all(dest)?;

    // Pass 1 — index every entry so we can resolve cross-page links.
    let mut index = build_link_index(&mut archive)?;

    // Strip the Notion top-level directory if all entries share a common
    // root (Notion exports are typically wrapped in a single folder).
    let strip = common_root(&index.entries);
    if !strip.is_empty() {
        index = index.strip_prefix(&strip);
    }

    // Pass 2 — walk and convert.
    let mut report = ImportReport::default();
    let entry_names: Vec<String> = index.entries.keys().cloned().collect();

    for name in entry_names {
        let original = format!("{strip}{name}");
        let mut zf = archive
            .by_name(&original)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string()))?;
        if zf.is_dir() {
            continue;
        }
        let mut buf = Vec::with_capacity(zf.size() as usize);
        zf.read_to_end(&mut buf)?;

        let target_rel = index.target_for(&name);
        let target_abs = dest.join(&target_rel);
        if let Some(parent) = target_abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let target_abs = unique_path(&target_abs);

        if name.ends_with(".md") {
            let body_str = String::from_utf8_lossy(&buf).into_owned();
            let converted = convert_page(&body_str, &index, &name, &mut report);
            std::fs::write(&target_abs, converted)?;
            report.pages_written += 1;
        } else if name.ends_with(".csv") {
            let csv_str = String::from_utf8_lossy(&buf).into_owned();
            let base_name = target_rel
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("database");
            match database::csv_to_bases(&csv_str, base_name) {
                Ok(bases_toml) => {
                    let bases_path = target_abs.with_extension("bases");
                    let bases_path = unique_path(&bases_path);
                    std::fs::write(&bases_path, bases_toml)?;
                    report.bases_written += 1;
                    report
                        .written
                        .push(bases_path.strip_prefix(dest).unwrap_or(&bases_path).to_path_buf());

                    // Also keep the raw CSV as an attachment so users can
                    // re-derive the base if column inference was wrong.
                    std::fs::write(&target_abs, &buf)?;
                    report.attachments_copied += 1;
                }
                Err(e) => {
                    report
                        .warnings
                        .push(format!("csv→bases failed for {name}: {e}"));
                    std::fs::write(&target_abs, &buf)?;
                    report.attachments_copied += 1;
                }
            }
        } else {
            std::fs::write(&target_abs, &buf)?;
            report.attachments_copied += 1;
        }

        report
            .written
            .push(target_abs.strip_prefix(dest).unwrap_or(&target_abs).to_path_buf());
    }

    Ok(report)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Convert a single Notion page's body. Extracts properties → frontmatter,
/// rewrites links, normalizes callouts.
fn convert_page(
    raw: &str,
    index: &LinkIndex,
    source_name: &str,
    report: &mut ImportReport,
) -> String {
    // 1. Property table (if present) → frontmatter.
    let (props, body_after_props) = property::extract_property_table(raw);

    // 2. Markdown body conversions (links, callouts).
    let body_converted =
        markdown::convert_notion_markdown(&body_after_props, &index.link_rewrites);

    // 3. Synthesize frontmatter.
    let notion_id = filename::extract_uuid(source_name);
    let mut fm = props.unwrap_or_default();
    if let Some(uid) = notion_id {
        fm.entry("notion_id".to_string()).or_insert(uid);
    }

    if markdown::has_unconverted_warning_marker(&body_converted) {
        report
            .warnings
            .push(format!("unconverted block in {source_name}"));
    }

    serialize_with_frontmatter(&fm, &body_converted)
}

fn serialize_with_frontmatter(
    fm: &std::collections::BTreeMap<String, String>,
    body: &str,
) -> String {
    if fm.is_empty() {
        return body.to_string();
    }
    let mut out = String::from("---\n");
    for (k, v) in fm {
        // Quote anything with special YAML characters.
        let needs_quote =
            v.contains(':') || v.contains('#') || v.starts_with(' ') || v.contains('\n');
        if needs_quote {
            let escaped = v.replace('"', "\\\"");
            out.push_str(&format!("{k}: \"{escaped}\"\n"));
        } else {
            out.push_str(&format!("{k}: {v}\n"));
        }
    }
    out.push_str("---\n\n");
    out.push_str(body);
    out
}

fn unique_path(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
    let parent = p.parent().unwrap_or_else(|| Path::new(""));
    for n in 1..u32::MAX {
        let candidate = if ext.is_empty() {
            parent.join(format!("{stem} ({n})"))
        } else {
            parent.join(format!("{stem} ({n}).{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    p.to_path_buf()
}

/// Determine the common top-level prefix of all zip entries so it can be
/// stripped (Notion exports wrap everything in a single folder).
fn common_root(entries: &HashMap<String, PathBuf>) -> String {
    let mut iter = entries.keys();
    let first = match iter.next() {
        Some(s) => s,
        None => return String::new(),
    };
    let first_root = first.split('/').next().unwrap_or("").to_string();
    if first_root.is_empty() {
        return String::new();
    }
    for name in iter {
        if !name.starts_with(&format!("{first_root}/")) && name != &first_root {
            return String::new();
        }
    }
    format!("{first_root}/")
}

// ── Link index ──────────────────────────────────────────────────────────────

/// Cross-page link resolution table built in pass 1.
///
/// Maps each *original* Notion-encoded URL (`Page%20Title%20abc.md`) to
/// the destination wikilink target (`Page Title`), and each *original*
/// zip-entry name (`folder/Page Title abc.md`) to the cleaned destination
/// path (`folder/Page Title.md`).
struct LinkIndex {
    /// Source path → destination relative path.
    entries: HashMap<String, PathBuf>,
    /// URL-encoded source filename → display title (for wikilink rewrites).
    link_rewrites: HashMap<String, String>,
}

impl LinkIndex {
    fn target_for(&self, src: &str) -> PathBuf {
        self.entries
            .get(src)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(src))
    }

    fn strip_prefix(self, prefix: &str) -> Self {
        let prefix_path = std::path::Path::new(prefix.trim_end_matches('/'));
        let entries = self
            .entries
            .into_iter()
            .map(|(k, v)| {
                let stripped_k = k.strip_prefix(prefix).unwrap_or(&k).to_string();
                let stripped_v = v.strip_prefix(prefix_path).map(PathBuf::from).unwrap_or(v);
                (stripped_k, stripped_v)
            })
            .collect();
        Self {
            entries,
            link_rewrites: self.link_rewrites,
        }
    }
}

fn build_link_index<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<LinkIndex> {
    let mut entries: HashMap<String, PathBuf> = HashMap::new();
    let mut link_rewrites: HashMap<String, String> = HashMap::new();

    for i in 0..archive.len() {
        let zf = archive
            .by_index(i)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        let name = zf.name().to_string();
        if zf.is_dir() {
            continue;
        }
        let cleaned = filename::clean_path(&name);
        entries.insert(name.clone(), cleaned.clone());

        // Encoded forms users may have in Notion mention links.
        if name.ends_with(".md") {
            let basename = Path::new(&name)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&name);
            let title = filename::strip_notion_uuid(basename).0;
            let title_no_ext = title.trim_end_matches(".md").to_string();

            // The forms Notion uses in links: URL-encoded full path and
            // URL-encoded basename.
            link_rewrites.insert(percent_encode(basename), title_no_ext.clone());
            link_rewrites.insert(percent_encode(&name), title_no_ext);
        }
    }

    Ok(LinkIndex {
        entries,
        link_rewrites,
    })
}

/// Minimal percent-encoder for spaces and a handful of reserved chars.
/// Notion encodes spaces as `%20` and parens as `%28` / `%29`. We only need
/// to recognize the encoded form, so the decoder is the more interesting
/// half — but encoding here too makes the lookup table symmetric.
pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"),
                16,
            ) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn make_zip(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in files {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(body.as_bytes()).unwrap();
            }
            zw.finish().unwrap();
        }
        buf
    }

    #[test]
    fn import_round_trips_a_minimal_zip() {
        let zip = make_zip(&[(
            "Export/Page Title abc123def456abc123def456abc12345.md",
            "# Page Title\n\nHello, world.\n",
        )]);
        let dest = tempfile::tempdir().unwrap();
        let report =
            import_notion_archive(Cursor::new(zip), dest.path()).expect("import ok");

        assert_eq!(report.pages_written, 1);
        let page = std::fs::read_to_string(dest.path().join("Page Title.md")).unwrap();
        assert!(page.contains("notion_id: abc123def456abc123def456abc12345"));
        assert!(page.contains("Hello, world."));
    }

    #[test]
    fn import_rewrites_internal_mention_links() {
        let body_a =
            "# A\n\nSee [B](B%20bbb111bbb222bbb333bbb444bbb55555.md) for context.\n";
        let body_b = "# B\n\nThe second page.\n";
        let zip = make_zip(&[
            ("Export/A aaa111aaa222aaa333aaa444aaa55555.md", body_a),
            ("Export/B bbb111bbb222bbb333bbb444bbb55555.md", body_b),
        ]);
        let dest = tempfile::tempdir().unwrap();
        import_notion_archive(Cursor::new(zip), dest.path()).expect("import ok");

        let a = std::fs::read_to_string(dest.path().join("A.md")).unwrap();
        assert!(a.contains("[[B]]"), "expected wikilink to B, got:\n{a}");
    }

    #[test]
    fn import_extracts_callouts() {
        let zip = make_zip(&[(
            "Export/Page abcd1234abcd1234abcd1234abcd1234.md",
            "# Page\n\n> 💡 An info callout.\n\n> ⚠️ A warning.\n",
        )]);
        let dest = tempfile::tempdir().unwrap();
        import_notion_archive(Cursor::new(zip), dest.path()).expect("import ok");
        let page = std::fs::read_to_string(dest.path().join("Page.md")).unwrap();
        assert!(page.contains("> [!note] An info callout."), "{page}");
        assert!(page.contains("> [!warning] A warning."), "{page}");
    }

    #[test]
    fn import_extracts_property_table() {
        let body = "# Tasks\n\n| Status | In Progress |\n| --- | --- |\n| Owner | Alex |\n| Due | 2026-06-01 |\n\nBody starts here.\n";
        let zip = make_zip(&[(
            "Export/Tasks aaaa1111aaaa1111aaaa1111aaaa1111.md",
            body,
        )]);
        let dest = tempfile::tempdir().unwrap();
        import_notion_archive(Cursor::new(zip), dest.path()).expect("import ok");
        let page = std::fs::read_to_string(dest.path().join("Tasks.md")).unwrap();
        assert!(page.starts_with("---\n"), "{page}");
        assert!(page.contains("Status: In Progress"), "{page}");
        assert!(page.contains("Owner: Alex"), "{page}");
        assert!(page.contains("Due: 2026-06-01"), "{page}");
        assert!(page.contains("Body starts here."), "{page}");
    }

    #[test]
    fn import_converts_csv_to_bases_and_keeps_csv() {
        let csv = "Name,Status,Priority\nAlpha,Done,1\nBeta,WIP,2\n";
        let zip = make_zip(&[(
            "Export/My DB aaaa1111aaaa1111aaaa1111aaaa1111.csv",
            csv,
        )]);
        let dest = tempfile::tempdir().unwrap();
        let report =
            import_notion_archive(Cursor::new(zip), dest.path()).expect("import ok");
        assert_eq!(report.bases_written, 1);
        let bases = std::fs::read_to_string(dest.path().join("My DB.bases")).unwrap();
        assert!(bases.contains("[[fields]]"), "{bases}");
        assert!(bases.contains("Name"), "{bases}");
        assert!(dest.path().join("My DB.csv").exists());
    }

    #[test]
    fn percent_encode_decode_round_trip() {
        let original = "Page Title (with parens).md";
        let encoded = percent_encode(original);
        let decoded = percent_decode(&encoded);
        assert_eq!(decoded, original);
    }
}
