//! `nexus sandbox` — inspect the OS-sandbox policy and run brokered downloads.
//!
//! Thin proxy over `com.nexus.security`: `sandbox_policy` (introspection) and
//! `download` (the permissioned-download broker). See `docs/0.1.2/os-sandbox.md`.

use anyhow::{Context, Result};
use nexus_types::constants::IPC_TIMEOUT_EXTENDED as IPC_TIMEOUT;
use nexus_types::plugin_ids;
use serde_json::Value;

use crate::app::App;

const SECURITY_PLUGIN: &str = plugin_ids::SECURITY;

/// `nexus sandbox policy` — print the active OS-sandbox configuration.
///
/// # Errors
/// Returns an error if the IPC call fails.
pub(crate) fn policy(app: &mut App) -> Result<()> {
    let (invoker, rt) = app.invoker()?;
    let cfg: Value = rt
        .block_on(invoker.ipc_call(
            SECURITY_PLUGIN,
            "sandbox_policy",
            serde_json::json!({}),
            IPC_TIMEOUT,
        ))
        .context("security ipc call 'sandbox_policy' failed")?;
    println!(
        "{}",
        serde_json::to_string_pretty(&cfg).unwrap_or_else(|_| cfg.to_string())
    );
    Ok(())
}

/// `nexus sandbox download <url> <dest>` — brokered, allowlisted download.
///
/// # Errors
/// Returns an error if the IPC call fails or the broker refuses the request
/// (downloads disabled, host off the allowlist, dest outside a writable root,
/// size cap exceeded, …).
pub(crate) fn download(app: &mut App, url: &str, dest: &str, cwd: Option<&str>) -> Result<()> {
    let args = serde_json::json!({ "url": url, "dest": dest, "cwd": cwd });
    let (invoker, rt) = app.invoker()?;
    let res: Value = rt
        .block_on(invoker.ipc_call(SECURITY_PLUGIN, "download", args, IPC_TIMEOUT))
        .context("security ipc call 'download' failed")?;
    let bytes = res.get("bytes_written").and_then(Value::as_u64).unwrap_or(0);
    println!("downloaded {bytes} bytes to {dest}");
    Ok(())
}
