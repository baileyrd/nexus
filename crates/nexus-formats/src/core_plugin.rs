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
//!
//! Ids are append-only.
//!
//! Both handlers are blocking (they walk filesystems and parse files).
//! The kernel runs each dispatch on a dedicated thread, so the
//! synchronous design is fine.

use std::path::PathBuf;

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

// ── Handler ids ─────────────────────────────────────────────────────────────

/// Reverse-DNS plugin id.
pub const PLUGIN_ID: &str = "com.nexus.formats";

/// `import_notion` handler id.
pub const HANDLER_IMPORT_NOTION: u32 = 1;
/// `export_notion` handler id.
pub const HANDLER_EXPORT_NOTION: u32 = 2;
/// `export_html` handler id.
pub const HANDLER_EXPORT_HTML: u32 = 3;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::formats::register`.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("import_notion", HANDLER_IMPORT_NOTION),
    ("export_notion", HANDLER_EXPORT_NOTION),
    ("export_html", HANDLER_EXPORT_HTML),
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

    #[test]
    fn unknown_handler_id_errors() {
        let dir = tempdir().unwrap();
        let mut plugin = FormatsCorePlugin::open(dir.path().to_path_buf());
        let err = plugin.dispatch(99, &serde_json::json!({})).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("unknown handler"), "{msg}");
    }
}
