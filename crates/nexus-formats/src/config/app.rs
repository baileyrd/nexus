//! Application config (`app.toml`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level application settings loaded from `.forge/app.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    /// Core forge settings.
    pub core: CoreSettings,
    /// Editor behaviour.
    pub editor: EditorSettings,
    /// Preview rendering.
    pub preview: PreviewSettings,
    /// Search engine.
    pub search: SearchSettings,
    /// Plugin configuration.
    pub plugins: PluginSettings,
    /// Git integration.
    pub git: GitSettings,
    /// Dream Cycle (BL-129) — scheduled entity-graph maintenance.
    pub dream_cycle: DreamCycleSettings,
    /// Flat key/value bag mirrored by the shell's settings registry.
    /// Keys follow the `pluginId.fieldName` convention (e.g.
    /// `"nexus.editor.fontSize"`). Values can be any TOML scalar or
    /// table. The shell auto-persists into this map; CLI/TUI surfaces
    /// can read from the typed sections above or here, whichever fits.
    /// `BTreeMap` keeps the on-disk order stable so diffs read cleanly.
    pub settings: BTreeMap<String, toml::Value>,
}

/// Core forge settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CoreSettings {
    /// Display name.
    pub name: String,
    /// Default directory for new notes.
    pub default_note_dir: String,
    /// Directory for binary attachments.
    pub attachment_dir: String,
    /// Daily note title format string.
    pub daily_note_format: String,
    /// Default layout mode.
    pub default_layout: String,
    /// UI theme name.
    pub theme: String,
    /// UI language code.
    pub language: String,
}

impl Default for CoreSettings {
    fn default() -> Self {
        Self {
            name:               "MyForge".into(),
            default_note_dir:   "notes".into(),
            attachment_dir:     "attachments".into(),
            daily_note_format:  "%Y-%m-%d".into(),
            default_layout:     "sidebar".into(),
            theme:              "auto".into(),
            language:           "en".into(),
        }
    }
}

/// Editor behaviour settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// Font size in pixels.
    pub font_size: u32,
    /// Font family name.
    pub font_family: String,
    /// Line height multiplier.
    pub line_height: f64,
    /// Enable vim keybindings.
    pub enable_vim_mode: bool,
    /// Auto-save on change.
    pub auto_save: bool,
    /// Auto-save delay in milliseconds.
    pub auto_save_delay_ms: u64,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size:          14,
            font_family:        "monospace".into(),
            line_height:        1.6,
            enable_vim_mode:    false,
            auto_save:          true,
            auto_save_delay_ms: 3000,
        }
    }
}

/// Preview rendering settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct PreviewSettings {
    /// Enable Mermaid diagram rendering.
    pub enable_mermaid:    bool,
    /// Enable `KaTeX` math rendering.
    pub enable_katex:      bool,
    /// Enable syntax highlighting.
    pub enable_highlight:  bool,
    /// Resolve wikilinks in preview.
    pub enable_wikilinks:  bool,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self {
            enable_mermaid:   true,
            enable_katex:     true,
            enable_highlight: true,
            enable_wikilinks: true,
        }
    }
}

/// Search engine settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    /// Enable full-text search.
    pub enable_full_text:    bool,
    /// Re-index interval in milliseconds.
    pub index_interval_ms:   u64,
    /// Maximum results to return.
    pub max_results:         usize,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            enable_full_text:  true,
            index_interval_ms: 5000,
            max_results:       50,
        }
    }
}

/// Plugin configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PluginSettings {
    /// Enabled plugin IDs.
    pub enabled: Vec<String>,
}

/// Git integration settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct GitSettings {
    /// Whether git integration is active.
    pub enabled: bool,
    /// Enable automatic commits.
    pub auto_commit: bool,
    /// Auto-commit interval in seconds (`0` = disabled). Default: 1800.
    pub auto_commit_interval_secs: u64,
    /// Auto-commit on file-save events.
    pub auto_commit_on_save: bool,
    /// Debounce window in seconds for rapid saves. Default: 5.
    pub auto_commit_debounce_secs: u64,
    /// P2-06 — interval between `git status` polls (background state
    /// watcher). `None` ⇒ `nexus_git::core_plugin::DEFAULT_POLL_INTERVAL`
    /// (2 s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll_interval_secs: Option<u64>,
    /// P2-06 — wake-up cadence inside the auto-commit idle loop.
    /// `None` ⇒ `nexus_git::core_plugin::DEFAULT_AUTO_COMMIT_TICK` (30 s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_commit_tick_secs: Option<u64>,
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            enabled:                   true,
            auto_commit:               false,
            auto_commit_interval_secs: 1800,
            auto_commit_on_save:       false,
            auto_commit_debounce_secs: 5,
            poll_interval_secs:        None,
            auto_commit_tick_secs:     None,
        }
    }
}

/// Dream Cycle (BL-129) — scheduled entity-graph maintenance.
///
/// Four phases run in sequence (`dedup`, `enrich`, `decay`, `infer`)
/// against the BL-128 entity graph. The thin slice surfaces only
/// `dedup` + `decay`; the remaining phases land in the BL-129
/// close-out and inherit the same configuration block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DreamCycleSettings {
    /// When `false`, the cron trigger never fires and the CLI emits a
    /// `disabled` notice. Direct invocation via `nexus graph
    /// dream-cycle run --force` still works (close-out).
    pub enabled: bool,
    /// Cron expression for the scheduled trigger. Local time. The
    /// default (`"0 2 * * *"`) fires at 02:00 every day to match the
    /// Thoth reference behaviour.
    pub schedule: String,
    /// Auto-merge threshold for the `dedup` phase. Pairs whose Jaccard
    /// similarity meets or exceeds this value merge silently; values
    /// in `[review_threshold, merge_threshold)` are surfaced for
    /// review. Range `[0.0, 1.0]`; default `0.97`.
    pub merge_threshold: f32,
    /// Surface-for-review threshold for the `dedup` phase. Pairs at
    /// or above this value (and below `merge_threshold`) are returned
    /// to the CLI / shell as duplicate candidates. Range `[0.0, 1.0]`;
    /// default `0.92`.
    pub review_threshold: f32,
    /// Multiplicative decay factor applied to every relation
    /// confidence per cycle. Range `(0.0, 1.0]`; default `0.95`.
    /// Set to `1.0` to disable decay without disabling the cycle.
    pub decay_factor: f32,
    /// Lower bound for relation confidence post-decay. Relations
    /// already at or below the floor are skipped (no churn). Range
    /// `[0.0, 1.0]`; default `0.1`.
    pub decay_floor: f32,
}

impl Default for DreamCycleSettings {
    fn default() -> Self {
        Self {
            enabled:          false,
            schedule:         "0 2 * * *".into(),
            merge_threshold:  0.97,
            review_threshold: 0.92,
            decay_factor:     0.95,
            decay_floor:      0.10,
        }
    }
}
