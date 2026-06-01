//! Bases-domain handlers — schema/record/property/view CRUD, plus
//! `base_index`, `base_load`, `base_list`, `base_query`.
//!
//! The largest single domain in storage (17 handlers). Bases own the
//! tabular-view subsystem: a `.bases/` directory on disk is parsed
//! into a typed schema + record set, then indexed into SQLite for
//! filterable / sortable / paginated queries.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    StorageBaseCreateArgs, StorageBaseIndexResult, StorageBaseNamedArgs,
    StorageBasePropertyCreateArgs, StorageBasePropertyRenameArgs, StorageBasePropertyUpdateArgs,
    StorageBaseQueryArgs, StorageBaseRecordCreateArgs, StorageBaseRecordIdArgs,
    StorageBaseRecordUpdateArgs, StorageBaseViewArgs, StorageOk, StoragePathArgs,
};
use crate::StorageEngine;

use super::shared::{exec_err, parse_args, to_value};

// ── Records ─────────────────────────────────────────────────────────────────

pub(crate) fn record_create(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse the outer envelope via
    // `StorageBaseRecordCreateArgs`. The inner `record` is still a
    // `serde_json::Value` because `BaseRecord` isn't `JsonSchema`-
    // derive friendly yet; the existing `from_value::<BaseRecord>`
    // call below surfaces malformed inner shapes as a handler-side
    // parse error.
    let StorageBaseRecordCreateArgs { path, record } = parse_args(args, "base_record_create")?;
    let record: nexus_types::bases::BaseRecord = serde_json::from_value(record)
        .map_err(|e| exec_err(format!("base_record_create: record decode: {e}")))?;
    let stored = engine
        .base_record_create(&path, record)
        .map_err(|e| exec_err(format!("base_record_create: {e}")))?;
    to_value(&stored, "base_record_create")
}

pub(crate) fn record_update(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageBaseRecordUpdateArgs`.
    let StorageBaseRecordUpdateArgs {
        path,
        record_id,
        fields,
    } = parse_args(args, "base_record_update")?;
    let updated = engine
        .base_record_update(&path, &record_id, &fields)
        .map_err(|e| exec_err(format!("base_record_update: {e}")))?;
    to_value(&updated, "base_record_update")
}

pub(crate) fn record_delete(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageBaseRecordIdArgs` +
    // `StorageOk` reply.
    let StorageBaseRecordIdArgs { path, record_id } = parse_args(args, "base_record_delete")?;
    engine
        .base_record_delete(&path, &record_id)
        .map_err(|e| exec_err(format!("base_record_delete: {e}")))?;
    to_value(&StorageOk { ok: true }, "base_record_delete")
}

pub(crate) fn record_soft_delete(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    let StorageBaseRecordIdArgs { path, record_id } = parse_args(args, "base_record_soft_delete")?;
    engine
        .base_record_soft_delete(&path, &record_id)
        .map_err(|e| exec_err(format!("base_record_soft_delete: {e}")))?;
    to_value(&StorageOk { ok: true }, "base_record_soft_delete")
}

pub(crate) fn record_restore(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageBaseRecordIdArgs { path, record_id } = parse_args(args, "base_record_restore")?;
    engine
        .base_record_restore(&path, &record_id)
        .map_err(|e| exec_err(format!("base_record_restore: {e}")))?;
    to_value(&StorageOk { ok: true }, "base_record_restore")
}

// ── Properties ──────────────────────────────────────────────────────────────

pub(crate) fn property_create(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageBasePropertyCreateArgs`.
    let StorageBasePropertyCreateArgs {
        path,
        name,
        definition,
    } = parse_args(args, "base_property_create")?;
    engine
        .base_property_create(&path, &name, definition)
        .map_err(|e| exec_err(format!("base_property_create: {e}")))?;
    to_value(&StorageOk { ok: true }, "base_property_create")
}

pub(crate) fn property_update(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageBasePropertyUpdateArgs`.
    let StorageBasePropertyUpdateArgs {
        path,
        name,
        definition,
        migrate_values,
    } = parse_args(args, "base_property_update")?;
    engine
        .base_property_update(&path, &name, &definition, migrate_values)
        .map_err(|e| exec_err(format!("base_property_update: {e}")))?;
    to_value(&StorageOk { ok: true }, "base_property_update")
}

pub(crate) fn property_delete(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via the shared `StorageBaseNamedArgs`.
    let StorageBaseNamedArgs { path, name } = parse_args(args, "base_property_delete")?;
    engine
        .base_property_delete(&path, &name)
        .map_err(|e| exec_err(format!("base_property_delete: {e}")))?;
    to_value(&StorageOk { ok: true }, "base_property_delete")
}

pub(crate) fn property_rename(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageBasePropertyRenameArgs`.
    let StorageBasePropertyRenameArgs {
        path,
        old_name,
        new_name,
    } = parse_args(args, "base_property_rename")?;
    engine
        .base_property_rename(&path, &old_name, &new_name)
        .map_err(|e| exec_err(format!("base_property_rename: {e}")))?;
    to_value(&StorageOk { ok: true }, "base_property_rename")
}

// ── Views ───────────────────────────────────────────────────────────────────

pub(crate) fn view_create(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via the shared `StorageBaseViewArgs`.
    let StorageBaseViewArgs { path, view } = parse_args(args, "base_view_create")?;
    let view: nexus_types::bases::BaseView = serde_json::from_value(view)
        .map_err(|e| exec_err(format!("base_view_create: view decode: {e}")))?;
    engine
        .base_view_create(&path, view)
        .map_err(|e| exec_err(format!("base_view_create: {e}")))?;
    to_value(&StorageOk { ok: true }, "base_view_create")
}

pub(crate) fn view_update(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageBaseViewArgs { path, view } = parse_args(args, "base_view_update")?;
    let view: nexus_types::bases::BaseView = serde_json::from_value(view)
        .map_err(|e| exec_err(format!("base_view_update: view decode: {e}")))?;
    engine
        .base_view_update(&path, view)
        .map_err(|e| exec_err(format!("base_view_update: {e}")))?;
    to_value(&StorageOk { ok: true }, "base_view_update")
}

pub(crate) fn view_delete(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — shares `StorageBaseNamedArgs` with `property_delete`.
    let StorageBaseNamedArgs { path, name } = parse_args(args, "base_view_delete")?;
    engine
        .base_view_delete(&path, &name)
        .map_err(|e| exec_err(format!("base_view_delete: {e}")))?;
    to_value(&StorageOk { ok: true }, "base_view_delete")
}

// ── Base lifecycle + load/list/query ────────────────────────────────────────

pub(crate) fn create(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageBaseCreateArgs`.
    let StorageBaseCreateArgs {
        path,
        schema,
        seed_records,
    } = parse_args(args, "base_create")?;
    let schema: nexus_types::bases::BaseSchema = serde_json::from_value(schema)
        .map_err(|e| exec_err(format!("base_create: schema decode: {e}")))?;
    let seed_records: Vec<nexus_types::bases::BaseRecord> =
        serde_json::from_value(Value::Array(seed_records))
            .map_err(|e| exec_err(format!("base_create: seed_records decode: {e}")))?;
    let base = engine
        .base_create(&path, &schema, seed_records)
        .map_err(|e| exec_err(format!("base_create: {e}")))?;
    to_value(&base, "base_create")
}

pub(crate) fn index(
    engine: &StorageEngine,
    forge_root: &Path,
    args: &Value,
) -> Result<Value, PluginError> {
    // #190 / R7 — `base_index` takes a plain `{ path }`; use the
    // shared `StoragePathArgs`. Reply is the new typed
    // `StorageBaseIndexResult { base_id }`.
    let StoragePathArgs { path } = parse_args(args, "base_index")?;
    let abs_dir = forge_root.join(&path);
    let base = nexus_types::bases::load_base(&abs_dir)
        .map_err(|e| exec_err(format!("base_index: load: {e}")))?;
    let base_id = engine
        .index_base(&path, &base)
        .map_err(|e| exec_err(format!("base_index: {e}")))?;
    to_value(&StorageBaseIndexResult { base_id }, "base_index")
}

pub(crate) fn load(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — `base_load` takes a plain `{ path }`.
    let StoragePathArgs { path } = parse_args(args, "base_load")?;
    let abs_dir = forge_root.join(&path);
    let base =
        nexus_types::bases::load_base(&abs_dir).map_err(|e| exec_err(format!("base_load: {e}")))?;
    to_value(&base, "base_load")
}

pub(crate) fn list(engine: &StorageEngine) -> Result<Value, PluginError> {
    let bases = engine
        .list_bases()
        .map_err(|e| exec_err(format!("base_list: {e}")))?;
    to_value(&bases, "base_list")
}

pub(crate) fn query(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via `StorageBaseQueryArgs`.
    let StorageBaseQueryArgs {
        path,
        filters,
        sorts,
        limit,
        offset,
    } = parse_args(args, "base_query")?;

    let bases = engine
        .list_bases()
        .map_err(|e| exec_err(format!("base_query: list_bases: {e}")))?;
    let base_summary = bases
        .iter()
        .find(|b| b.path == path)
        .ok_or_else(|| exec_err(format!("base_query: base not found: {path}")))?;

    let mut db_query = crate::bases::query::Query {
        base_id: base_summary.id,
        ..Default::default()
    };
    for f in &filters {
        db_query.filters.push(
            crate::bases::query::parse_filter(f)
                .map_err(|e| exec_err(format!("base_query: parse filter '{f}': {e}")))?,
        );
    }
    for s in &sorts {
        db_query.sorts.push(
            crate::bases::query::parse_sort(s)
                .map_err(|e| exec_err(format!("base_query: parse sort '{s}': {e}")))?,
        );
    }
    db_query.limit = limit;
    db_query.offset = offset;

    let conn = engine
        .pool_connection()
        .map_err(|e| exec_err(format!("base_query: pool: {e}")))?;
    let result = crate::bases::query::execute(&conn, &db_query)
        .map_err(|e| exec_err(format!("base_query: {e}")))?;
    to_value(&result, "base_query")
}
