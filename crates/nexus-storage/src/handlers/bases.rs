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

use crate::StorageEngine;

use super::shared::{exec_err, name_arg, path_arg, to_value};

// ── Records ─────────────────────────────────────────────────────────────────

pub(crate) fn record_create(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_record_create")?;
    let record: nexus_types::bases::BaseRecord = args
        .get("record")
        .ok_or_else(|| exec_err("base_record_create: missing 'record'".to_string()))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("base_record_create: record decode: {e}")))
        })?;
    let stored = engine
        .base_record_create(&path, record)
        .map_err(|e| exec_err(format!("base_record_create: {e}")))?;
    to_value(&stored, "base_record_create")
}

pub(crate) fn record_update(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_record_update")?;
    let record_id = args
        .get("record_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("base_record_update: missing 'record_id' string".to_string()))?;
    let fields = args
        .get("fields")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .ok_or_else(|| exec_err("base_record_update: missing 'fields' object".to_string()))?;
    let updated = engine
        .base_record_update(&path, record_id, &fields)
        .map_err(|e| exec_err(format!("base_record_update: {e}")))?;
    to_value(&updated, "base_record_update")
}

pub(crate) fn record_delete(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_record_delete")?;
    let record_id = args
        .get("record_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("base_record_delete: missing 'record_id' string".to_string()))?;
    engine
        .base_record_delete(&path, record_id)
        .map_err(|e| exec_err(format!("base_record_delete: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn record_soft_delete(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_record_soft_delete")?;
    let record_id = args
        .get("record_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            exec_err("base_record_soft_delete: missing 'record_id' string".to_string())
        })?;
    engine
        .base_record_soft_delete(&path, record_id)
        .map_err(|e| exec_err(format!("base_record_soft_delete: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn record_restore(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_record_restore")?;
    let record_id = args
        .get("record_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("base_record_restore: missing 'record_id' string".to_string()))?;
    engine
        .base_record_restore(&path, record_id)
        .map_err(|e| exec_err(format!("base_record_restore: {e}")))?;
    Ok(serde_json::json!({}))
}

// ── Properties ──────────────────────────────────────────────────────────────

pub(crate) fn property_create(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_property_create")?;
    let name = name_arg(args, "base_property_create")?;
    let definition = args
        .get("definition")
        .cloned()
        .ok_or_else(|| exec_err("base_property_create: missing 'definition'".to_string()))?;
    engine
        .base_property_create(&path, &name, definition)
        .map_err(|e| exec_err(format!("base_property_create: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn property_update(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_property_update")?;
    let name = name_arg(args, "base_property_update")?;
    let definition = args
        .get("definition")
        .cloned()
        .ok_or_else(|| exec_err("base_property_update: missing 'definition'".to_string()))?;
    let migrate_values = args
        .get("migrate_values")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    engine
        .base_property_update(&path, &name, &definition, migrate_values)
        .map_err(|e| exec_err(format!("base_property_update: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn property_delete(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_property_delete")?;
    let name = name_arg(args, "base_property_delete")?;
    engine
        .base_property_delete(&path, &name)
        .map_err(|e| exec_err(format!("base_property_delete: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn property_rename(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_property_rename")?;
    let old_name = args
        .get("old_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("base_property_rename: missing 'old_name' string".to_string()))?
        .to_string();
    let new_name = args
        .get("new_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("base_property_rename: missing 'new_name' string".to_string()))?
        .to_string();
    engine
        .base_property_rename(&path, &old_name, &new_name)
        .map_err(|e| exec_err(format!("base_property_rename: {e}")))?;
    Ok(serde_json::json!({}))
}

// ── Views ───────────────────────────────────────────────────────────────────

pub(crate) fn view_create(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_view_create")?;
    let view: nexus_types::bases::BaseView = args
        .get("view")
        .ok_or_else(|| exec_err("base_view_create: missing 'view'".to_string()))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("base_view_create: view decode: {e}")))
        })?;
    engine
        .base_view_create(&path, view)
        .map_err(|e| exec_err(format!("base_view_create: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn view_update(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_view_update")?;
    let view: nexus_types::bases::BaseView = args
        .get("view")
        .ok_or_else(|| exec_err("base_view_update: missing 'view'".to_string()))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("base_view_update: view decode: {e}")))
        })?;
    engine
        .base_view_update(&path, view)
        .map_err(|e| exec_err(format!("base_view_update: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn view_delete(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_view_delete")?;
    let name = name_arg(args, "base_view_delete")?;
    engine
        .base_view_delete(&path, &name)
        .map_err(|e| exec_err(format!("base_view_delete: {e}")))?;
    Ok(serde_json::json!({}))
}

// ── Base lifecycle + load/list/query ────────────────────────────────────────

pub(crate) fn create(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_create")?;
    let schema: nexus_types::bases::BaseSchema = args
        .get("schema")
        .ok_or_else(|| exec_err("base_create: missing 'schema'".to_string()))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("base_create: schema decode: {e}")))
        })?;
    let seed_records: Vec<nexus_types::bases::BaseRecord> = args
        .get("seed_records")
        .cloned()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| exec_err(format!("base_create: seed_records decode: {e}")))
        })
        .transpose()?
        .unwrap_or_default();
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
    let path = path_arg(args, "base_index")?;
    let abs_dir = forge_root.join(&path);
    let base = nexus_types::bases::load_base(&abs_dir)
        .map_err(|e| exec_err(format!("base_index: load: {e}")))?;
    let base_id = engine
        .index_base(&path, &base)
        .map_err(|e| exec_err(format!("base_index: {e}")))?;
    Ok(serde_json::json!({ "base_id": base_id }))
}

pub(crate) fn load(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_load")?;
    let abs_dir = forge_root.join(&path);
    let base = nexus_types::bases::load_base(&abs_dir)
        .map_err(|e| exec_err(format!("base_load: {e}")))?;
    to_value(&base, "base_load")
}

pub(crate) fn list(engine: &StorageEngine) -> Result<Value, PluginError> {
    let bases = engine
        .list_bases()
        .map_err(|e| exec_err(format!("base_list: {e}")))?;
    to_value(&bases, "base_list")
}

pub(crate) fn query(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "base_query")?;
    let filters: Vec<String> = args
        .get("filters")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let sorts: Vec<String> = args
        .get("sorts")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let limit = args
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok());
    let offset = args
        .get("offset")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok());

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
