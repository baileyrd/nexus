//! # nexus-formats
//!
//! File-format library for Nexus (PRD 06).
//!
//! Provides pure-Rust parsers and serializers for Nexus file formats plus
//! forge configuration files. No runtime services; no `SQLite`.
//!
//! Bases types live in [`nexus_types::bases`](../../nexus_types/bases/index.html)
//! — the active runtime consumers (database/CLI/storage) build on that type
//! hierarchy, not on this crate.
//!
//! ## Formats
//!
//! | Module | Format | Files |
//! |--------|--------|-------|
//! | [`markdown`] | CommonMark + GFM + Nexus extensions | `.md`, `.mdx` |
//! | [`canvas`]   | Obsidian-compatible canvas (v1.0) | `.canvas` |
//! | [`config`]   | Forge configuration | `app.toml`, `workspace.json`, … |
//! | [`util`]     | Slug generation, filename validation, attachment naming | — |

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod canvas;
pub mod config;
pub mod core_plugin;
pub mod error;
pub mod markdown;
pub mod migration;
pub mod notion;
pub mod util;

pub use core_plugin::{FormatsCorePlugin, HANDLER_EXPORT_NOTION, HANDLER_IMPORT_NOTION, PLUGIN_ID};

// ── Convenience re-exports ────────────────────────────────────────────────────

pub use error::{CanvasError, ConfigError, Error, MarkdownError, Result, UtilError};

pub use util::{
    attachment_name, detect_mime, sha256_hex, slugify, validate_filename, validate_path,
};

pub use canvas::{
    CanvasBackground, CanvasEdge, CanvasEdgeType, CanvasFile, CanvasNode, CanvasNodeType,
};
pub use markdown::export_to_html;

pub use config::{
    load_ai_config, load_app_config, load_mcp_config, load_workspace_state, save_ai_config,
    save_app_config, save_mcp_config, save_workspace_state, AiConfig, AppConfig, McpConfig,
    WorkspaceState,
};

pub use markdown::{
    parse as parse_markdown, parse_frontmatter, resolve_wikilink, Block, BlockKind, Frontmatter,
    LinkType, MathSpan, ParsedMarkdown, Tag, TagSource, Task, WikiLink,
};

pub use migration::{
    detect_version, scan_versions, FormatVersion, MigrationError, MigrationFn, MigrationRegistry,
    VersionTally, DEFAULT_VERSION,
};
