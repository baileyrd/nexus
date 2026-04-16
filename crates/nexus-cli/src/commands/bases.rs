//! CLI commands for bases (database) operations.

use anyhow::Result;
use nexus_bootstrap::{database as database_ipc, storage as storage_ipc};
use nexus_types::bases;

use crate::app::App;
use crate::output;

/// Create a new base with the given schema JSON.
pub fn create(app: &mut App, path: &str, schema_json: &str) -> Result<()> {
    let schema: bases::BaseSchema = serde_json::from_str(schema_json)
        .map_err(|e| anyhow::anyhow!("Invalid schema JSON: {e}"))?;

    let abs_dir = app.forge_root().join(path);
    let field_count = schema.fields.len();
    bases::init_base(&abs_dir, path, &schema)?;

    let (runtime, rt) = app.runtime()?;
    storage_ipc::base_index(runtime, rt, path)?;

    output::print_success(
        app.format(),
        &format!("Created base: {path} ({field_count} fields)"),
        &serde_json::json!(null),
    );
    Ok(())
}

/// List all indexed bases.
pub fn list(app: &mut App) -> Result<()> {
    let (runtime, rt) = app.runtime()?;
    let bases = storage_ipc::base_list(runtime, rt)?;
    if bases.is_empty() {
        println!("No bases found.");
        return Ok(());
    }
    for b in &bases {
        println!("{} — {} ({} records)", b.path, b.name, b.record_count);
    }
    Ok(())
}

/// Show details of a base.
pub fn show(app: &mut App, path: &str) -> Result<()> {
    let abs_dir = app.forge_root().join(path);
    let base = bases::load_base(&abs_dir)?;
    println!("Base: {}", base.name);
    println!("Fields:");
    for (name, def) in &base.schema.fields {
        let ft = def.get("type").and_then(|v| v.as_str()).unwrap_or("?");
        let req = def.get("required").and_then(|v| v.as_bool()).unwrap_or(false);
        let marker = if req { " (required)" } else { "" };
        println!("  {name}: {ft}{marker}");
    }
    println!("Records: {}", base.records.len());
    println!("Views: {}", base.views.len());
    println!("Relations: {}", base.relations.len());
    Ok(())
}

/// Add a record to a base from JSON.
pub fn add_record(app: &mut App, path: &str, data_json: &str) -> Result<()> {
    let record: bases::BaseRecord = serde_json::from_str(data_json)
        .map_err(|e| anyhow::anyhow!("Invalid record JSON: {e}"))?;

    let abs_dir = app.forge_root().join(path);
    let mut base = bases::load_base(&abs_dir)?;
    bases::validate_record(&base.schema, &record)?;

    base.records.push(record);
    bases::save_base(&abs_dir, &base)?;

    let record_count = base.records.len();
    let (runtime, rt) = app.runtime()?;
    storage_ipc::base_index(runtime, rt, path)?;

    output::print_success(
        app.format(),
        &format!("Added record to {path} (total: {record_count})"),
        &serde_json::json!(null),
    );
    Ok(())
}

/// Query records from a base with optional filters, sorts, and pagination.
pub fn query(
    app: &mut App,
    path: &str,
    filters: &[String],
    sorts: &[String],
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<()> {
    // Fast path: no filters/sorts/pagination → read from disk directly.
    if filters.is_empty() && sorts.is_empty() && limit.is_none() && offset.is_none() {
        let abs_dir = app.forge_root().join(path);
        let base = bases::load_base(&abs_dir)?;
        if base.records.is_empty() {
            println!("No records in {path}.");
            return Ok(());
        }
        for record in &base.records {
            println!("{}", serde_json::to_string(record)?);
        }
        return Ok(());
    }

    // Structured query — delegate to storage plugin (which holds the DB
    // connection) via IPC.
    let (runtime, rt) = app.runtime()?;
    let result = storage_ipc::base_query(runtime, rt, path, filters, sorts, limit, offset)?;

    if result.records.is_empty() {
        println!("No matching records.");
    } else {
        for record in &result.records {
            println!("{}", serde_json::to_string(record)?);
        }
        println!("--- {} of {} total", result.records.len(), result.total_count);
        if result.has_more {
            println!("(more records available — use --offset to paginate)");
        }
    }
    Ok(())
}

/// Import records from a CSV file.
pub fn import(app: &mut App, path: &str, csv_file: &str, has_header: bool) -> Result<()> {
    let abs_dir = app.forge_root().join(path);
    let mut base = bases::load_base(&abs_dir)?;

    let csv_bytes = std::fs::read(csv_file)
        .map_err(|e| anyhow::anyhow!("Failed to read CSV file: {e}"))?;
    let field_names: Vec<String> = base.schema.fields.keys().cloned().collect();

    let (runtime, rt) = app.runtime()?;
    let response = database_ipc::csv_import(
        runtime,
        rt,
        &csv_bytes,
        &field_names,
        has_header,
        None,
    )?;

    base.records.extend(response.records);
    bases::save_base(&abs_dir, &base)?;

    storage_ipc::base_index(runtime, rt, path)?;

    println!(
        "Imported {} records ({} skipped)",
        response.imported, response.skipped
    );
    for (row, err) in &response.errors {
        println!("  Row {row}: {err}");
    }
    Ok(())
}

/// Export records to a CSV file.
pub fn export(app: &mut App, path: &str, csv_file: &str) -> Result<()> {
    let abs_dir = app.forge_root().join(path);
    let base = bases::load_base(&abs_dir)?;

    let field_names: Vec<String> = base.schema.fields.keys().cloned().collect();

    let (runtime, rt) = app.runtime()?;
    let response = database_ipc::csv_export(runtime, rt, &base.records, &field_names)?;

    std::fs::write(csv_file, &response.csv_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to write CSV file: {e}"))?;

    println!("Exported {} records to {csv_file}", response.count);
    Ok(())
}

/// Evaluate a formula against a specific record.
pub fn formula(app: &mut App, path: &str, record_id: &str, expr: &str) -> Result<()> {
    let abs_dir = app.forge_root().join(path);
    let base = bases::load_base(&abs_dir)?;

    let record = base
        .records
        .iter()
        .find(|r| r.id == record_id)
        .ok_or_else(|| anyhow::anyhow!("Record not found: {record_id}"))?;

    let (runtime, rt) = app.runtime()?;
    let response = database_ipc::formula_eval(runtime, rt, expr, &record.fields)?;
    println!("{}", response.display);
    Ok(())
}
