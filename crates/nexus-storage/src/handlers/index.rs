//! Index/rebuild + import + Obsidian-base handlers: `rebuild_index`,
//! `rebuild_search_index`, `import_forge`, `obsidian_base_query`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::StorageEngine;

use super::shared::{exec_err, path_arg, to_value};

pub(crate) fn rebuild_index(engine: &StorageEngine) -> Result<Value, PluginError> {
    let stats = engine
        .rebuild_index()
        .map_err(|e| exec_err(format!("rebuild_index: {e}")))?;
    to_value(&stats, "rebuild_index")
}

pub(crate) fn rebuild_search_index(engine: &StorageEngine) -> Result<Value, PluginError> {
    engine
        .rebuild_search_index()
        .map_err(|e| exec_err(format!("rebuild_search_index: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn obsidian_base_query(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    let path = path_arg(args, "obsidian_base_query")?;
    let result = engine
        .obsidian_base_query(&path)
        .map_err(|e| exec_err(format!("obsidian_base_query '{path}': {e}")))?;
    to_value(&result, "obsidian_base_query")
}

pub(crate) fn import_forge(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let source = args
        .get("source")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("import_forge: missing 'source' string argument".to_string()))?
        .to_string();
    let source_path = Path::new(&source);
    let dry_run = args
        .get("dry_run")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let on_conflict = match args
        .get("on_conflict")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("skip")
    {
        "overwrite" => crate::import::ConflictStrategy::Overwrite,
        "rename" => crate::import::ConflictStrategy::Rename,
        _ => crate::import::ConflictStrategy::Skip,
    };

    let plan = engine
        .plan_import(source_path)
        .map_err(|e| exec_err(format!("import_forge plan from '{source}': {e}")))?;
    if dry_run {
        return to_value(&plan, "import_forge");
    }
    let report = engine
        .apply_import(
            source_path,
            &plan,
            &crate::import::ImportOptions { on_conflict },
        )
        .map_err(|e| exec_err(format!("import_forge apply from '{source}': {e}")))?;
    // Rebuild the destination index so the imported files surface in
    // search / graph.
    let _ = engine.rebuild_index();
    to_value(&report, "import_forge")
}
