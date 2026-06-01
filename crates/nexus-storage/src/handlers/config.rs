//! Config + settings handlers: `config_read`, `config_reset`,
//! `settings_read`, `settings_write`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    StorageConfigContentResult, StorageConfigKindArgs, StorageOk, StorageSettingsWriteArgs,
};

use super::shared::{exec_err, parse_args, to_value};

pub(crate) fn read(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageConfigKindArgs`; reply
    // via typed `StorageConfigContentResult`.
    let StorageConfigKindArgs { kind } = parse_args(args, "config_read")?;
    let (format, content) = match kind.as_str() {
        "app" => {
            let cfg = crate::config::load_app_config(forge_root)
                .map_err(|e| exec_err(format!("config_read: {e}")))?;
            (
                "toml",
                toml::to_string_pretty(&cfg)
                    .map_err(|e| exec_err(format!("config_read: serialize app: {e}")))?,
            )
        }
        "workspace" => {
            let state = crate::config::load_workspace_state(forge_root)
                .map_err(|e| exec_err(format!("config_read: {e}")))?;
            (
                "json",
                serde_json::to_string_pretty(&state)
                    .map_err(|e| exec_err(format!("config_read: serialize workspace: {e}")))?,
            )
        }
        "mcp" => {
            let cfg = crate::config::load_mcp_config(forge_root)
                .map_err(|e| exec_err(format!("config_read: {e}")))?;
            (
                "toml",
                toml::to_string_pretty(&cfg)
                    .map_err(|e| exec_err(format!("config_read: serialize mcp: {e}")))?,
            )
        }
        "ai" => {
            let cfg = crate::config::load_ai_config(forge_root)
                .map_err(|e| exec_err(format!("config_read: {e}")))?;
            (
                "toml",
                toml::to_string_pretty(&cfg)
                    .map_err(|e| exec_err(format!("config_read: serialize ai: {e}")))?,
            )
        }
        other => {
            return Err(exec_err(format!(
                "config_read: unknown kind '{other}' (expected app|workspace|mcp|ai)"
            )))
        }
    };
    to_value(
        &StorageConfigContentResult {
            format: format.to_string(),
            content,
        },
        "config_read",
    )
}

pub(crate) fn reset(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageConfigKindArgs`.
    let StorageConfigKindArgs { kind } = parse_args(args, "config_reset")?;
    match kind.as_str() {
        "app" => crate::config::save_app_config(forge_root, &crate::config::AppConfig::default())
            .map_err(|e| exec_err(format!("config_reset: {e}")))?,
        "workspace" => crate::config::save_workspace_state(
            forge_root,
            &crate::config::WorkspaceState::default(),
        )
        .map_err(|e| exec_err(format!("config_reset: {e}")))?,
        "mcp" => crate::config::save_mcp_config(forge_root, &crate::config::McpConfig::default())
            .map_err(|e| exec_err(format!("config_reset: {e}")))?,
        "ai" => crate::config::save_ai_config(forge_root, &crate::config::AiConfig::default())
            .map_err(|e| exec_err(format!("config_reset: {e}")))?,
        other => {
            return Err(exec_err(format!(
                "config_reset: unknown kind '{other}' (expected app|workspace|mcp|ai)"
            )))
        }
    }
    to_value(&StorageOk { ok: true }, "config_reset")
}

pub(crate) fn settings_read(forge_root: &Path) -> Result<Value, PluginError> {
    let cfg = crate::config::load_app_config(forge_root)
        .map_err(|e| exec_err(format!("settings_read: {e}")))?;
    // toml::Value implements Serialize, so serde_json walks the tree
    // and produces a JSON object directly. No manual conversion.
    serde_json::to_value(&cfg.settings)
        .map_err(|e| exec_err(format!("settings_read: serialize: {e}")))
}

pub(crate) fn settings_write(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageSettingsWriteArgs`.
    // `value: null` removes the key; any other JSON value (scalar
    // or object) round-trips through `toml::Value`.
    let StorageSettingsWriteArgs { key, value } = parse_args(args, "settings_write")?;
    let mut cfg = crate::config::load_app_config(forge_root)
        .map_err(|e| exec_err(format!("settings_write: load: {e}")))?;

    if value.is_null() {
        cfg.settings.remove(&key);
    } else {
        let toml_value: toml::Value = serde_json::from_value(value)
            .map_err(|e| exec_err(format!("settings_write: value→toml: {e}")))?;
        cfg.settings.insert(key, toml_value);
    }

    crate::config::save_app_config(forge_root, &cfg)
        .map_err(|e| exec_err(format!("settings_write: save: {e}")))?;
    to_value(&StorageOk { ok: true }, "settings_write")
}
