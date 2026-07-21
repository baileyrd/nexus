//! Core plugin exposing format-conversion handlers over kernel IPC.
//!
//! Initial surface: Notion zip-import and Notion-format export. Both
//! handlers are best-effort wrappers around the pure-library functions
//! in [`crate::notion`] — every conversion happens server-side; the
//! caller only supplies paths and receives a summary report.
//!
//! # Handlers
//!
//! | Id | Command         | Args                                                          | Purpose                          |
//! |---:|-----------------|----------------------------------------------------------------|----------------------------------|
//! | 1  | `import_notion` | `{ source: PathBuf, dest?: PathBuf }`                           | Import a Notion zip-export.      |
//! | 2  | `export_notion` | `{ source?: PathBuf, dest: PathBuf }`                           | Export a forge subdirectory.     |
//! | 3  | `export_html`   | `{ source: PathBuf, title?: String, dest?: PathBuf }`           | Render a note to standalone HTML.|
//! | 4  | `export_pandoc` | `{ source: PathBuf, format: "docx" \| "odt", dest: PathBuf }`   | Word/ODT export via pandoc (C69, #422).|
//!
//! Ids are append-only.
//!
//! Both handlers are blocking (they walk filesystems and parse files).
//! The kernel runs each dispatch on a dedicated thread, so the
//! synchronous design is fine.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

// ── IPC arg types ───────────────────────────────────────────────────────────

/// Args for `com.nexus.formats::import_notion` (handler `1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ImportNotionArgs {
    /// Absolute path to the Notion zip export.
    pub source: PathBuf,
    /// Destination directory. Forge-relative if not absolute. Defaults to
    /// `Imported from Notion` under the forge root.
    #[serde(default)]
    pub dest: Option<PathBuf>,
}

/// Args for `com.nexus.formats::export_notion` (handler `2`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ExportNotionArgs {
    /// Forge-relative subdirectory to export. Defaults to the entire forge.
    #[serde(default)]
    pub source: Option<PathBuf>,
    /// Output directory. Created if missing.
    pub dest: PathBuf,
}

/// Args for `com.nexus.formats::export_html` (handler `3`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ExportHtmlArgs {
    /// Forge-relative (or absolute) path to the markdown note to render.
    pub source: PathBuf,
    /// Document title. Defaults to the source file's stem.
    #[serde(default)]
    pub title: Option<String>,
    /// Forge-relative (or absolute) output path. When given, the HTML is
    /// written there and the response reports `{ written: true, dest }`.
    /// When omitted the rendered HTML is returned inline as `{ html }`.
    #[serde(default)]
    pub dest: Option<PathBuf>,
}

/// Output format for `export_pandoc` (C69, #422).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "snake_case")]
pub enum PandocFormat {
    /// Microsoft Word (`.docx`).
    Docx,
    /// `OpenDocument` Text (`.odt`).
    Odt,
}

impl PandocFormat {
    /// The `-t` value pandoc expects for this format.
    #[must_use]
    pub fn pandoc_target(self) -> &'static str {
        match self {
            PandocFormat::Docx => "docx",
            PandocFormat::Odt => "odt",
        }
    }
}

/// Args for `com.nexus.formats::export_pandoc` (handler `4`, C69 #422).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ExportPandocArgs {
    /// Forge-relative (or absolute) path to the markdown note to convert.
    pub source: PathBuf,
    /// Output format.
    pub format: PandocFormat,
    /// Forge-relative (or absolute) output path. Parent directories are
    /// created if missing.
    pub dest: PathBuf,
}

// ── Handler ids ─────────────────────────────────────────────────────────────

/// Reverse-DNS plugin id.
pub const PLUGIN_ID: &str = "com.nexus.formats";

/// `import_notion` handler id.
pub const HANDLER_IMPORT_NOTION: u32 = 1;
/// `export_notion` handler id.
pub const HANDLER_EXPORT_NOTION: u32 = 2;
/// `export_html` handler id.
pub const HANDLER_EXPORT_HTML: u32 = 3;
/// `export_pandoc` handler id (C69, #422).
pub const HANDLER_EXPORT_PANDOC: u32 = 4;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::formats::register`.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("import_notion", HANDLER_IMPORT_NOTION),
    ("export_notion", HANDLER_EXPORT_NOTION),
    ("export_html", HANDLER_EXPORT_HTML),
    ("export_pandoc", HANDLER_EXPORT_PANDOC),
];

// ── Plugin ──────────────────────────────────────────────────────────────────

/// Core plugin holding the forge root for path resolution.
pub struct FormatsCorePlugin {
    forge_root: PathBuf,
}

impl FormatsCorePlugin {
    /// Build a plugin against the given forge root.
    #[must_use]
    pub fn open(forge_root: PathBuf) -> Self {
        Self { forge_root }
    }
}

impl CorePlugin for FormatsCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_IMPORT_NOTION => self.dispatch_import_notion(args),
            HANDLER_EXPORT_NOTION => self.dispatch_export_notion(args),
            HANDLER_EXPORT_HTML => self.dispatch_export_html(args),
            HANDLER_EXPORT_PANDOC => self.dispatch_export_pandoc(args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

impl FormatsCorePlugin {
    fn dispatch_import_notion(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ImportNotionArgs = parse_args(args, "import_notion")?;
        if !a.source.exists() {
            return Err(exec_err(format!(
                "source zip not found: {}",
                a.source.display()
            )));
        }
        let dest_abs = match a.dest {
            Some(d) if d.is_absolute() => d,
            Some(d) => self.forge_root.join(d),
            None => self.forge_root.join("Imported from Notion"),
        };
        let report = crate::notion::import_notion_zip(&a.source, &dest_abs)
            .map_err(|e| exec_err(format!("import_notion: {e}")))?;
        Ok(serde_json::json!({
            "pages_written": report.pages_written,
            "bases_written": report.bases_written,
            "attachments_copied": report.attachments_copied,
            "warnings": report.warnings,
            "dest": dest_abs.display().to_string(),
        }))
    }

    fn dispatch_export_notion(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ExportNotionArgs = parse_args(args, "export_notion")?;
        let source_abs = match a.source {
            Some(s) if s.is_absolute() => s,
            Some(s) => self.forge_root.join(s),
            None => self.forge_root.clone(),
        };
        if !source_abs.is_dir() {
            return Err(exec_err(format!(
                "source is not a directory: {}",
                source_abs.display()
            )));
        }
        let report = crate::notion::export_to_notion(&source_abs, &a.dest)
            .map_err(|e| exec_err(format!("export_notion: {e}")))?;
        Ok(serde_json::json!({
            "pages_written": report.pages_written,
            "databases_written": report.databases_written,
            "attachments_copied": report.attachments_copied,
            "warnings": report.warnings,
            "dest": a.dest.display().to_string(),
        }))
    }

    fn dispatch_export_html(&self, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        let a: ExportHtmlArgs = parse_args(args, "export_html")?;
        let source_abs = if a.source.is_absolute() {
            a.source.clone()
        } else {
            self.forge_root.join(&a.source)
        };
        let content = std::fs::read_to_string(&source_abs)
            .map_err(|e| exec_err(format!("export_html: failed to read {}: {e}", source_abs.display())))?;
        let title = a.title.clone().unwrap_or_else(|| {
            a.source
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string()
        });
        let html = crate::markdown::export_to_html(&content, &title);

        match a.dest {
            Some(dest) => {
                let dest_abs = if dest.is_absolute() {
                    dest
                } else {
                    self.forge_root.join(dest)
                };
                if let Some(parent) = dest_abs.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        exec_err(format!("export_html: failed to create {}: {e}", parent.display()))
                    })?;
                }
                std::fs::write(&dest_abs, &html).map_err(|e| {
                    exec_err(format!("export_html: failed to write {}: {e}", dest_abs.display()))
                })?;
                Ok(serde_json::json!({
                    "written": true,
                    "dest": dest_abs.display().to_string(),
                }))
            }
            None => Ok(serde_json::json!({ "html": html })),
        }
    }

    /// C69 (#422) — pandoc-backed Word/ODT export. Mirrors the LSP/DAP
    /// external-tool precedent (`Command::new` + let the OS resolve
    /// `PATH`, map `ErrorKind::NotFound` to a clear message) rather than
    /// a separate "is pandoc installed" pre-check: spawning is the
    /// detection.
    ///
    /// Feeds the raw markdown to pandoc's stdin (`-f markdown`) rather
    /// than passing `source` as a file argument, so this works
    /// identically whether pandoc supports the exact CLI path handling
    /// or not. Wikilinks/callouts are not resolved before conversion —
    /// same documented first-cut limitation as `export_html`.
    fn dispatch_export_pandoc(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ExportPandocArgs = parse_args(args, "export_pandoc")?;
        let source_abs = if a.source.is_absolute() {
            a.source.clone()
        } else {
            self.forge_root.join(&a.source)
        };
        let content = std::fs::read_to_string(&source_abs).map_err(|e| {
            exec_err(format!(
                "export_pandoc: failed to read {}: {e}",
                source_abs.display()
            ))
        })?;

        let dest_abs = if a.dest.is_absolute() {
            a.dest.clone()
        } else {
            self.forge_root.join(&a.dest)
        };
        if let Some(parent) = dest_abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                exec_err(format!(
                    "export_pandoc: failed to create {}: {e}",
                    parent.display()
                ))
            })?;
        }

        run_pandoc("pandoc", &content, a.format.pandoc_target(), &dest_abs)?;

        Ok(serde_json::json!({
            "written": true,
            "dest": dest_abs.display().to_string(),
        }))
    }
}

/// Spawn `pandoc_command` (always `"pandoc"` in production; overridable
/// in tests so the not-found path can be exercised deterministically
/// without mutating the process-wide `PATH`) to convert `content` to
/// `format_target`, writing the result to `dest`.
fn run_pandoc(
    pandoc_command: &str,
    content: &str,
    format_target: &str,
    dest: &std::path::Path,
) -> Result<(), PluginError> {
    let mut child = Command::new(pandoc_command)
        .args(["-f", "markdown", "-t", format_target, "-o"])
        .arg(dest)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                exec_err(
                    "export_pandoc: 'pandoc' not found on PATH — install pandoc \
                     (https://pandoc.org/installing.html) to use docx/odt export"
                        .to_string(),
                )
            } else {
                exec_err(format!("export_pandoc: failed to spawn pandoc: {e}"))
            }
        })?;

    // Scoped so `stdin` drops (closing the pipe) before
    // `wait_with_output` — pandoc reads stdin until EOF.
    {
        let mut stdin = child
            .stdin
            .take()
            .expect("stdin was configured as piped above");
        stdin
            .write_all(content.as_bytes())
            .map_err(|e| exec_err(format!("export_pandoc: failed to write to pandoc stdin: {e}")))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| exec_err(format!("export_pandoc: failed waiting for pandoc: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(exec_err(format!(
            "export_pandoc: pandoc exited with {}: {}",
            output.status,
            stderr.trim()
        )));
    }
    Ok(())
}

// ── Plumbing — SD-01: helpers emitted by the shared macro ───────────────────

nexus_plugins::define_dispatch_helpers!();

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    fn make_zip(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in files {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(body.as_bytes()).unwrap();
            }
            zw.finish().unwrap();
        }
        buf
    }

    #[test]
    fn import_notion_dispatches_through_ipc() {
        let dir = tempdir().unwrap();
        let zip = make_zip(&[(
            "Export/Page abcd1234abcd1234abcd1234abcd1234.md",
            "# Page\n\nBody.\n",
        )]);
        let zip_path = dir.path().join("export.zip");
        std::fs::write(&zip_path, zip).unwrap();

        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let result = plugin
            .dispatch(
                HANDLER_IMPORT_NOTION,
                &serde_json::json!({ "source": zip_path, "dest": "from-notion" }),
            )
            .unwrap();
        assert_eq!(result["pages_written"].as_u64(), Some(1));
        assert!(dir.path().join("from-notion/Page.md").exists());
    }

    #[test]
    fn import_notion_with_missing_source_errors() {
        let dir = tempdir().unwrap();
        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let err = plugin
            .dispatch(
                HANDLER_IMPORT_NOTION,
                &serde_json::json!({ "source": dir.path().join("nope.zip") }),
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("not found"), "{msg}");
    }

    #[test]
    fn export_notion_round_trips_a_page() {
        let dir = tempdir().unwrap();
        let dest = tempdir().unwrap();
        std::fs::write(
            dir.path().join("Hello.md"),
            "---\nnotion_id: aaaa1111aaaa1111aaaa1111aaaa1111\n---\n\nBody.\n",
        )
        .unwrap();

        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let result = plugin
            .dispatch(
                HANDLER_EXPORT_NOTION,
                &serde_json::json!({ "dest": dest.path() }),
            )
            .unwrap();
        assert_eq!(result["pages_written"].as_u64(), Some(1));
        assert!(dest
            .path()
            .join("Hello aaaa1111aaaa1111aaaa1111aaaa1111.md")
            .exists());
    }

    #[test]
    fn export_html_returns_inline_html_by_default() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Hello.md"), "# Hello\n\nWorld\n").unwrap();

        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let result = plugin
            .dispatch(HANDLER_EXPORT_HTML, &serde_json::json!({ "source": "Hello.md" }))
            .unwrap();
        let html = result["html"].as_str().unwrap();
        assert!(html.contains("<h1>Hello</h1>"), "{html}");
        assert!(html.contains("<title>Hello</title>"), "{html}");
        assert!(html.contains("<!DOCTYPE html>"), "{html}");
    }

    #[test]
    fn export_html_writes_to_dest_when_given() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Note.md"), "# Note\n\nBody.\n").unwrap();

        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let result = plugin
            .dispatch(
                HANDLER_EXPORT_HTML,
                &serde_json::json!({ "source": "Note.md", "title": "Custom Title", "dest": "out/note.html" }),
            )
            .unwrap();
        assert_eq!(result["written"].as_bool(), Some(true));
        let written = std::fs::read_to_string(dir.path().join("out/note.html")).unwrap();
        assert!(written.contains("<title>Custom Title</title>"), "{written}");
        assert!(written.contains("Body."), "{written}");
    }

    #[test]
    fn export_html_with_missing_source_errors() {
        let dir = tempdir().unwrap();
        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let err = plugin
            .dispatch(HANDLER_EXPORT_HTML, &serde_json::json!({ "source": "nope.md" }))
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("failed to read"), "{msg}");
    }

    /// C69 (#422) — best-effort: pandoc isn't guaranteed to be on the
    /// runner's `PATH`. Skips rather than fails when it isn't, mirroring
    /// how the LSP/DAP test suites treat optional external tools.
    fn pandoc_available() -> bool {
        Command::new("pandoc")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[test]
    fn export_pandoc_writes_a_real_docx_when_pandoc_is_on_path() {
        if !pandoc_available() {
            eprintln!("skipping export_pandoc_writes_a_real_docx_when_pandoc_is_on_path: pandoc not on PATH");
            return;
        }
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Note.md"), "# Note\n\nSome body text.\n").unwrap();

        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let result = plugin
            .dispatch(
                HANDLER_EXPORT_PANDOC,
                &serde_json::json!({ "source": "Note.md", "format": "docx", "dest": "out/note.docx" }),
            )
            .unwrap();
        assert_eq!(result["written"].as_bool(), Some(true));
        let bytes = std::fs::read(dir.path().join("out/note.docx")).unwrap();
        // .docx is a zip container — check the magic bytes rather than
        // parsing the OOXML, which would need a whole extra dependency
        // just for this test.
        assert_eq!(&bytes[0..2], b"PK", "docx output should be a zip archive");
    }

    #[test]
    fn export_pandoc_writes_a_real_odt_when_pandoc_is_on_path() {
        if !pandoc_available() {
            eprintln!("skipping export_pandoc_writes_a_real_odt_when_pandoc_is_on_path: pandoc not on PATH");
            return;
        }
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Note.md"), "# Note\n\nSome body text.\n").unwrap();

        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let result = plugin
            .dispatch(
                HANDLER_EXPORT_PANDOC,
                &serde_json::json!({ "source": "Note.md", "format": "odt", "dest": "note.odt" }),
            )
            .unwrap();
        assert_eq!(result["written"].as_bool(), Some(true));
        let bytes = std::fs::read(dir.path().join("note.odt")).unwrap();
        assert_eq!(&bytes[0..2], b"PK", "odt output should be a zip archive");
    }

    #[test]
    fn export_pandoc_with_missing_source_errors() {
        let dir = tempdir().unwrap();
        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let err = plugin
            .dispatch(
                HANDLER_EXPORT_PANDOC,
                &serde_json::json!({ "source": "nope.md", "format": "docx", "dest": "out.docx" }),
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("failed to read"), "{msg}");
    }

    #[test]
    fn export_pandoc_rejects_an_unknown_format_string() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Note.md"), "# Note\n").unwrap();
        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let err = plugin
            .dispatch(
                HANDLER_EXPORT_PANDOC,
                &serde_json::json!({ "source": "Note.md", "format": "pdf", "dest": "out.pdf" }),
            )
            .unwrap_err();
        // `format` only accepts "docx" | "odt" — an unknown value fails
        // to deserialize before pandoc is ever spawned.
        assert!(format!("{err}").contains("export_pandoc"));
    }

    #[test]
    fn run_pandoc_surfaces_a_clear_error_when_the_binary_is_missing() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.docx");
        let err =
            run_pandoc("nexus-test-nonexistent-pandoc-binary", "# Note\n", "docx", &dest)
                .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("not found on PATH"), "{msg}");
        assert!(msg.contains("pandoc.org/installing"), "{msg}");
    }

    #[test]
    fn unknown_handler_id_errors() {
        let dir = tempdir().unwrap();
        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let err = plugin.dispatch(99, &serde_json::json!({})).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("unknown handler"), "{msg}");
    }
}
