//! `set_config` IPC handler — live-update the in-memory `AiConfig`
//! for chat and/or embedding. The shell pushes this on every boot
//! from its persisted config store so a user who set `provider=ollama`
//! once gets it back on the next launch without re-typing.

use std::sync::{Arc, RwLock};

use nexus_plugins::PluginError;

use crate::config::AiConfig;
use crate::handlers::shared::{config_snapshot, exec_err};

/// Live-update the in-memory `AiConfig` for chat and/or embedding.
///
/// Args shape:
///
/// ```json
/// {
///   "ai":        { "provider": "anthropic", "api_key": "...", "model": null, "base_url": null } | null,
///   "embedding": { "provider": "openai",    "api_key": "...", "model": null, "base_url": null } | null
/// }
/// ```
///
/// Field-level rules:
///   - `provider` is required when the side is present and non-null.
///     An empty string clears that side (same as passing `null`).
///   - `api_key` / `model` / `base_url` are optional; absent → `None`.
///   - An absent top-level key (no `"ai"` field at all) leaves that
///     side untouched.
pub(crate) fn handle_set_config(
    ai_handle: &Arc<RwLock<Option<AiConfig>>>,
    embed_handle: &Arc<RwLock<Option<AiConfig>>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let obj = args
        .as_object()
        .ok_or_else(|| exec_err("set_config: expected JSON object".to_string()))?;

    if obj.contains_key("ai") {
        let next = parse_config_field(obj.get("ai").unwrap_or(&serde_json::Value::Null))?;
        let mut g = ai_handle
            .write()
            .map_err(|_| exec_err("set_config: ai config lock poisoned".to_string()))?;
        *g = next;
    }
    if obj.contains_key("embedding") {
        let next = parse_config_field(obj.get("embedding").unwrap_or(&serde_json::Value::Null))?;
        let mut g = embed_handle
            .write()
            .map_err(|_| exec_err("set_config: embedding config lock poisoned".to_string()))?;
        *g = next;
    }

    let ai_view = ai_handle.read().ok().and_then(|g| g.clone());
    let embed_view = embed_handle.read().ok().and_then(|g| g.clone());
    Ok(config_snapshot(ai_view.as_ref(), embed_view.as_ref()))
}

/// Decode one side of the `set_config` payload. `Null` and a missing /
/// empty `provider` both mean "clear this side" — that's the path the
/// shell uses when the user blanks out the provider dropdown in
/// Settings → AI.
pub(crate) fn parse_config_field(
    value: &serde_json::Value,
) -> Result<Option<AiConfig>, PluginError> {
    if value.is_null() {
        return Ok(None);
    }
    let obj = value.as_object().ok_or_else(|| {
        exec_err("set_config: provider config must be object or null".to_string())
    })?;
    let provider = obj
        .get("provider")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if provider.is_empty() {
        return Ok(None);
    }
    let model = obj
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let api_key = obj
        .get("api_key")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let base_url = obj
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let local_embedding_model = if provider == "local" {
        model.clone()
    } else {
        None
    };
    let str_field = |k: &str| {
        obj.get(k)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
    };
    let anthropic_model = str_field("anthropic_model");
    let openai_chat_model = str_field("openai_chat_model");
    let openai_embedding_model = str_field("openai_embedding_model");
    let ollama_chat_model = str_field("ollama_chat_model");
    let ollama_embedding_model = str_field("ollama_embedding_model");
    let ollama_temperature = obj
        .get("ollama_temperature")
        .and_then(serde_json::Value::as_f64)
        .map(|v| v as f32);
    Ok(Some(AiConfig {
        provider,
        model,
        api_key,
        base_url,
        local_embedding_model,
        anthropic_model,
        openai_chat_model,
        openai_embedding_model,
        ollama_chat_model,
        ollama_embedding_model,
        ollama_temperature,
        ..AiConfig::default()
    }))
}
