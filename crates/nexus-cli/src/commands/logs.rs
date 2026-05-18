use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use nexus_types::constants::IPC_TIMEOUT_SHORT as IPC_TIMEOUT;
use nexus_types::plugin_ids;
use serde_json::{json, Value};

use crate::app::App;

const SECURITY_PLUGIN: &str = plugin_ids::SECURITY;
const MS_PER_DAY: i64 = 86_400_000;

/// Return the path to the logs directory.
fn logs_dir(app: &App) -> PathBuf {
    app.forge_root().join(".forge").join("logs")
}

/// Stream the most recent log entries, optionally filtered by `level`.
pub fn tail(app: &App, level: Option<&str>, lines: usize) -> Result<()> {
    let dir = logs_dir(app);
    if !dir.exists() {
        println!("No log files found.");
        return Ok(());
    }

    // Collect .log files and sort by name (lexicographic == date order for
    // nexus-YYYY-MM-DD.log naming).
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("log"))
        .collect();

    if entries.is_empty() {
        println!("No log files found.");
        return Ok(());
    }

    entries.sort();
    let most_recent = entries.last().expect("non-empty");

    let content = fs::read_to_string(most_recent)?;
    let all_lines: Vec<&str> = content.lines().collect();

    let tail_lines: Vec<&&str> = all_lines.iter().rev().take(lines).collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    let level_upper = level.map(|l| l.to_uppercase());

    for line in tail_lines {
        if let Some(ref lvl) = level_upper {
            if !line.to_uppercase().contains(lvl.as_str()) {
                continue;
            }
        }
        println!("{line}");
    }

    Ok(())
}

/// Show logs for the given `date` (YYYY-MM-DD format).
pub fn show(app: &App, date: &str) -> Result<()> {
    let log_file = logs_dir(app).join(format!("nexus-{date}.log"));
    if !log_file.exists() {
        anyhow::bail!("no log file for date: {date}");
    }
    let content = fs::read_to_string(&log_file)?;
    print!("{content}");
    Ok(())
}

/// Print the path to the log directory.
pub fn path(app: &App) -> Result<()> {
    println!("{}", logs_dir(app).display());
    Ok(())
}

// ── Audit log subcommands (BL-100) ────────────────────────────────────────────
//
// `tail` / `show` / `path` above operate on tracing log files written by
// the binary. The commands below query the SQLite audit store backing
// `nexus_kernel::audit::*` events through the `com.nexus.security`
// IPC handlers added in BL-094 / BL-100.

fn call_security(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (invoker, rt) = app.invoker()?;
    rt.block_on(invoker.ipc_call(SECURITY_PLUGIN, command, args, IPC_TIMEOUT))
        .with_context(|| format!("audit ipc call '{command}' failed"))
}

/// Parse an ISO date (YYYY-MM-DD) or RFC3339 datetime into Unix-millis (UTC).
fn parse_ts(s: &str) -> Result<i64> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp_millis());
    }
    let date = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("invalid date '{s}' (expected YYYY-MM-DD or RFC3339)"))?;
    let dt = date
        .and_hms_opt(0, 0, 0)
        .expect("00:00:00 is valid")
        .and_utc();
    Ok(dt.timestamp_millis())
}

fn format_ts(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| ms.to_string())
}

/// `nexus logs list` — query the audit store, newest first.
pub fn audit_list(
    app: &mut App,
    plugin: Option<String>,
    event_type: Option<String>,
    since: Option<String>,
    limit: u32,
) -> Result<()> {
    let since_ts = since.as_deref().map(parse_ts).transpose()?;
    let args = json!({
        "plugin_id": plugin,
        "event_type": event_type,
        "since_ts": since_ts,
        "limit": limit,
    });
    let entries = call_security(app, "query_audit_log", args)?;
    let rows = entries.as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        println!("No audit entries match.");
        return Ok(());
    }
    for row in rows {
        let ts = row.get("ts_ms").and_then(Value::as_i64).unwrap_or(0);
        let event = row.get("event_type").and_then(Value::as_str).unwrap_or("?");
        let plugin = row.get("plugin_id").and_then(Value::as_str).unwrap_or("-");
        let detail = row.get("detail_json").and_then(Value::as_str).unwrap_or("{}");
        println!("{}  {:<24}  {:<32}  {}", format_ts(ts), event, plugin, detail);
    }
    Ok(())
}

/// `nexus logs export` — dump audit entries as JSONL or CSV.
pub fn audit_export(
    app: &mut App,
    start: Option<String>,
    end: Option<String>,
    format: &str,
) -> Result<()> {
    let since_ts = start.as_deref().map(parse_ts).transpose()?;
    let end_ts = end.as_deref().map(parse_ts).transpose()?;

    // The store query supports `since_ts` natively; we filter `end_ts`
    // client-side. Pull a generous batch — export is not a paginated API.
    let args = json!({
        "since_ts": since_ts,
        "limit": 100_000,
    });
    let entries = call_security(app, "query_audit_log", args)?;
    let rows = entries.as_array().cloned().unwrap_or_default();
    let rows: Vec<Value> = rows
        .into_iter()
        .filter(|r| match end_ts {
            Some(end) => r.get("ts_ms").and_then(Value::as_i64).is_some_and(|t| t < end),
            None => true,
        })
        .collect();

    match format {
        "jsonl" => {
            for row in rows {
                println!("{}", serde_json::to_string(&row)?);
            }
        }
        "csv" => {
            println!("ts_ms,iso_ts,event_type,plugin_id,detail_json");
            for row in rows {
                let ts = row.get("ts_ms").and_then(Value::as_i64).unwrap_or(0);
                let event = row.get("event_type").and_then(Value::as_str).unwrap_or("");
                let plugin = row.get("plugin_id").and_then(Value::as_str).unwrap_or("");
                let detail = row.get("detail_json").and_then(Value::as_str).unwrap_or("");
                println!(
                    "{ts},{},{},{},{}",
                    format_ts(ts),
                    csv_escape(event),
                    csv_escape(plugin),
                    csv_escape(detail),
                );
            }
        }
        other => anyhow::bail!("unknown format '{other}' (expected 'jsonl' or 'csv')"),
    }
    Ok(())
}

/// `nexus logs clear` — prune entries older than `older_than` days.
pub fn audit_clear(app: &mut App, older_than: u32) -> Result<()> {
    let cutoff =
        chrono::Utc::now().timestamp_millis() - i64::from(older_than) * MS_PER_DAY;
    let response = call_security(app, "clear_audit_log", json!({ "before_ts": cutoff }))?;
    let removed = response.get("removed").and_then(Value::as_u64).unwrap_or(0);
    println!("Removed {removed} audit entries older than {older_than} days.");
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
