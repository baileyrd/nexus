//! Index/rebuild + import + Obsidian-base handlers: `rebuild_index`,
//! `rebuild_search_index`, `import_forge`, `obsidian_base_query`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    StorageImportConflictStrategy, StorageImportForgeArgs, StorageOk, StoragePathArgs,
};
use crate::StorageEngine;

use super::shared::{exec_err, parse_args, to_value};

pub(crate) fn rebuild_index(engine: &StorageEngine) -> Result<Value, PluginError> {
    let stats = engine
        .rebuild_index()
        .map_err(|e| exec_err(format!("rebuild_index: {e}")))?;
    to_value(&stats, "rebuild_index")
}

pub(crate) fn rebuild_search_index(engine: &StorageEngine) -> Result<Value, PluginError> {
    // #190 / R7 — typed reply via `StorageOk` (was `json!({})`).
    engine
        .rebuild_search_index()
        .map_err(|e| exec_err(format!("rebuild_search_index: {e}")))?;
    to_value(&StorageOk { ok: true }, "rebuild_search_index")
}

pub(crate) fn obsidian_base_query(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via the shared `StoragePathArgs`.
    let StoragePathArgs { path } = parse_args(args, "obsidian_base_query")?;
    let result = engine
        .obsidian_base_query(&path)
        .map_err(|e| exec_err(format!("obsidian_base_query '{path}': {e}")))?;
    to_value(&result, "obsidian_base_query")
}

pub(crate) fn import_forge(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageImportForgeArgs`. The
    // typed `StorageImportConflictStrategy` mirror replaces the
    // ad-hoc string-match on `on_conflict`; unknown values now
    // fail at the typed-parse boundary instead of silently
    // defaulting to `Skip`.
    let StorageImportForgeArgs {
        source,
        dry_run,
        on_conflict,
    } = parse_args(args, "import_forge")?;
    let source_path = Path::new(&source);
    let on_conflict = match on_conflict {
        StorageImportConflictStrategy::Skip => crate::import::ConflictStrategy::Skip,
        StorageImportConflictStrategy::Overwrite => crate::import::ConflictStrategy::Overwrite,
        StorageImportConflictStrategy::Rename => crate::import::ConflictStrategy::Rename,
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
