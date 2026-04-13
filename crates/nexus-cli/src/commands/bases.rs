//! CLI commands for bases (database) operations.

use anyhow::Result;

use crate::app::App;
use crate::output;

/// Create a new base with the given schema JSON.
pub fn create(app: &mut App, path: &str, schema_json: &str) -> Result<()> {
    let schema: nexus_storage::bases::BaseSchema =
        serde_json::from_str(schema_json).map_err(|e| anyhow::anyhow!("Invalid schema JSON: {e}"))?;

    let abs_dir = app.forge_root().join(path);
    let base = nexus_storage::bases::init_base(&abs_dir, path, &schema)?;

    // Index in SQLite.
    app.storage_mut()?.index_base(path, &base)?;

    output::print_success(
        app.format(),
        &format!("Created base: {path} ({} fields)", schema.fields.len()),
        &serde_json::json!(null),
    );
    Ok(())
}

/// List all indexed bases.
pub fn list(app: &mut App) -> Result<()> {
    let bases = app.storage_mut()?.list_bases()?;
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
    let base = nexus_storage::bases::load_base(&abs_dir)?;
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
    let record: nexus_storage::bases::BaseRecord =
        serde_json::from_str(data_json).map_err(|e| anyhow::anyhow!("Invalid record JSON: {e}"))?;

    let abs_dir = app.forge_root().join(path);
    let mut base = nexus_storage::bases::load_base(&abs_dir)?;

    // Validate against schema.
    nexus_storage::bases::validate_record(&base.schema, &record)?;

    base.records.push(record);
    nexus_storage::bases::save_base(&abs_dir, &base)?;

    // Re-index.
    app.storage_mut()?.index_base(path, &base)?;

    output::print_success(
        app.format(),
        &format!("Added record to {path} (total: {})", base.records.len()),
        &serde_json::json!(null),
    );
    Ok(())
}

/// Query records from a base.
pub fn query(app: &mut App, path: &str) -> Result<()> {
    let abs_dir = app.forge_root().join(path);
    let base = nexus_storage::bases::load_base(&abs_dir)?;
    if base.records.is_empty() {
        println!("No records in {path}.");
        return Ok(());
    }
    for record in &base.records {
        println!("{}", serde_json::to_string(record)?);
    }
    Ok(())
}
