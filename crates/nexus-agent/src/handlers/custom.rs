//! `com.nexus.agent::list_custom` (HANDLER_LIST_CUSTOM).
//!
//! Scans `<forge>/.forge/agents/*/agent.toml` and returns parsed
//! manifests + per-file parse errors.

use std::sync::Arc;

use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::PluginError;

pub(crate) async fn handle_list_custom(
    ctx: Arc<KernelPluginContext>,
) -> Result<serde_json::Value, PluginError> {
    let agents_dir = std::path::Path::new(crate::custom_agent::AGENTS_DIR);
    let entries = match ctx.list_files(agents_dir).await {
        Ok(e) => e,
        Err(_) => {
            return Ok(serde_json::json!({
                "manifests": [],
                "errors": []
            }));
        }
    };

    let mut manifests: Vec<crate::CustomAgentManifest> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();

    for entry in entries {
        let slug = entry
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if slug.is_empty() {
            continue;
        }
        let manifest_path = entry.join(crate::custom_agent::MANIFEST_FILE_NAME);
        let body = match ctx.read_file(&manifest_path).await {
            Ok(bytes) => match std::str::from_utf8(&bytes) {
                Ok(s) => s.to_string(),
                Err(e) => {
                    errors.push(serde_json::json!({
                        "path": manifest_path.display().to_string(),
                        "error": format!("manifest not UTF-8: {e}"),
                    }));
                    continue;
                }
            },
            Err(_) => continue,
        };

        match crate::custom_agent::parse_str(&body, &slug, &manifest_path) {
            Ok(manifest) => manifests.push(manifest),
            Err(e) => errors.push(serde_json::json!({
                "path": manifest_path.display().to_string(),
                "error": format!("{e}"),
            })),
        }
    }

    manifests.sort_by(|a, b| a.slug.cmp(&b.slug));

    Ok(serde_json::json!({
        "manifests": manifests,
        "errors": errors,
    }))
}
