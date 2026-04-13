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

/// Query records from a base with optional filters, sorts, and pagination.
pub fn query(
    app: &mut App,
    path: &str,
    filters: &[String],
    sorts: &[String],
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<()> {
    // If no filters/sorts/pagination, fall back to simple file load.
    if filters.is_empty() && sorts.is_empty() && limit.is_none() && offset.is_none() {
        let abs_dir = app.forge_root().join(path);
        let base = nexus_storage::bases::load_base(&abs_dir)?;
        if base.records.is_empty() {
            println!("No records in {path}.");
            return Ok(());
        }
        for record in &base.records {
            println!("{}", serde_json::to_string(record)?);
        }
        return Ok(());
    }

    // Use the database engine's query system.
    let storage = app.storage_mut()?;
    let bases = storage.list_bases()?;
    let base_summary = bases
        .iter()
        .find(|b| b.path == path)
        .ok_or_else(|| anyhow::anyhow!("Base not found: {path}"))?;

    let mut db_query = nexus_database::Query {
        base_id: base_summary.id,
        ..Default::default()
    };
    for f in filters {
        db_query.filters.push(nexus_database::parse_filter(f)?);
    }
    for s in sorts {
        db_query.sorts.push(nexus_database::parse_sort(s)?);
    }
    db_query.limit = limit;
    db_query.offset = offset;

    let conn = storage.pool_connection()?;
    let result = nexus_database::execute_query(&conn, &db_query)?;

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
    let mut base = nexus_storage::bases::load_base(&abs_dir)?;

    let file = std::fs::File::open(csv_file)
        .map_err(|e| anyhow::anyhow!("Failed to open CSV file: {e}"))?;

    // Build column mapping from header names matching schema fields.
    let field_names: Vec<String> = base.schema.fields.keys().cloned().collect();
    let mapping = if has_header {
        // Peek at headers to build mapping.
        let mut peek_reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(std::io::BufReader::new(std::fs::File::open(csv_file)?));
        let headers = peek_reader.headers()?.clone();
        nexus_database::ColumnMapping::from_headers(&headers, &field_names)
    } else {
        // Map columns 0..N to fields in order.
        nexus_database::ColumnMapping {
            mappings: field_names.iter().enumerate().map(|(i, n)| (i, n.clone())).collect(),
        }
    };

    let (records, result) = nexus_database::import_csv(file, &mapping, has_header)?;
    base.records.extend(records);
    nexus_storage::bases::save_base(&abs_dir, &base)?;
    app.storage_mut()?.index_base(path, &base)?;

    println!(
        "Imported {} records ({} skipped)",
        result.imported, result.skipped
    );
    for (row, err) in &result.errors {
        println!("  Row {row}: {err}");
    }
    Ok(())
}

/// Export records to a CSV file.
pub fn export(app: &mut App, path: &str, csv_file: &str) -> Result<()> {
    let abs_dir = app.forge_root().join(path);
    let base = nexus_storage::bases::load_base(&abs_dir)?;

    let field_names: Vec<String> = base.schema.fields.keys().cloned().collect();
    let file = std::fs::File::create(csv_file)
        .map_err(|e| anyhow::anyhow!("Failed to create CSV file: {e}"))?;

    let count = nexus_database::export_csv(file, &base.records, &field_names)?;
    println!("Exported {count} records to {csv_file}");
    Ok(())
}

/// Evaluate a formula against a specific record.
pub fn formula(app: &mut App, path: &str, record_id: &str, expr: &str) -> Result<()> {
    let abs_dir = app.forge_root().join(path);
    let base = nexus_storage::bases::load_base(&abs_dir)?;

    let record = base
        .records
        .iter()
        .find(|r| r.id == record_id)
        .ok_or_else(|| anyhow::anyhow!("Record not found: {record_id}"))?;

    let result = nexus_database::evaluate_formula(expr, &record.fields)?;
    println!("{result}");
    Ok(())
}
