//! Tauri command bridge into the editor core plugin.
//!
//! All commands here are thin adapters: they serialize their args into
//! JSON and call [`nexus_kernel::PluginContext::ipc_call`] on the
//! kernel runtime held in Tauri state. The runtime is built once at
//! startup in [`crate::lib::run`] and panics gracefully (returning an
//! error from each command) if the initial forge bootstrap failed.
//!
//! The target plugin is `com.nexus.editor` (see
//! [`nexus_editor::core_plugin`](../../nexus-editor/src/core_plugin.rs)).

#![allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexus_kernel::PluginContext;
use tauri::State;

/// Reverse-DNS id of the editor core plugin. Duplicated from
/// `nexus-editor` to avoid pulling the whole editor crate into
/// `nexus-app`'s public surface.
const EDITOR_PLUGIN_ID: &str = "com.nexus.editor";

/// Default per-call timeout. Sessions are in-memory — editor handlers
/// never block on disk for long.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Tauri-managed handle to the assembled kernel runtime.
///
/// `None` when startup forge bootstrap failed — every editor command
/// short-circuits with a clear error in that case.
pub struct KernelRuntime(pub Mutex<Option<Arc<nexus_bootstrap::Runtime>>>);

impl KernelRuntime {
    /// Empty placeholder, installed before the forge/runtime is known.
    #[must_use]
    pub fn empty() -> Self {
        Self(Mutex::new(None))
    }

    /// Install the built runtime so subsequent commands can reach it.
    pub fn set(&self, runtime: Arc<nexus_bootstrap::Runtime>) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(runtime);
        }
    }

    /// Borrow the underlying kernel runtime. Returns an error when the
    /// forge bootstrap failed and no runtime was installed.
    pub fn snapshot(&self) -> Result<Arc<nexus_bootstrap::Runtime>, String> {
        self.0
            .lock()
            .map_err(|_| "kernel runtime lock poisoned".to_string())?
            .as_ref()
            .cloned()
            .ok_or_else(|| "kernel runtime unavailable (forge bootstrap failed)".to_string())
    }
}

async fn call_editor(
    runtime: State<'_, KernelRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let rt = runtime.snapshot()?;
    rt.context
        .ipc_call(EDITOR_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

/// Open a markdown file and create an in-memory editor session.
#[tauri::command]
pub async fn editor_open(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_editor(runtime, "open", serde_json::json!({ "relpath": relpath })).await
}

/// Close a session without writing changes to disk.
#[tauri::command]
pub async fn editor_close(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_editor(runtime, "close", serde_json::json!({ "relpath": relpath })).await
}

/// Fetch a snapshot of a currently-open session.
#[tauri::command]
pub async fn editor_get_tree(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_editor(
        runtime,
        "get_tree",
        serde_json::json!({ "relpath": relpath }),
    )
    .await
}

/// Serialize the in-memory tree back to disk.
#[tauri::command]
pub async fn editor_save(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_editor(runtime, "save", serde_json::json!({ "relpath": relpath })).await
}

/// Apply a serialized `Transaction` atomically.
#[tauri::command]
pub async fn editor_apply_transaction(
    relpath: String,
    transaction: serde_json::Value,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_editor(
        runtime,
        "apply_transaction",
        serde_json::json!({ "relpath": relpath, "transaction": transaction }),
    )
    .await
}

/// Undo the most-recent applied transaction.
#[tauri::command]
pub async fn editor_undo(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_editor(runtime, "undo", serde_json::json!({ "relpath": relpath })).await
}

/// Redo the most-recent undone transaction.
#[tauri::command]
pub async fn editor_redo(
    relpath: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_editor(runtime, "redo", serde_json::json!({ "relpath": relpath })).await
}

/// List forge-relative paths of every session currently open.
#[tauri::command]
pub async fn editor_list_open(
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_editor(runtime, "list_open", serde_json::json!({})).await
}

/// Update the in-memory block tree from raw markdown text without disk I/O.
///
/// Called by the frontend after a debounce period following each keystroke so
/// that consumers (AI, MCP, outline) always read a reasonably fresh block tree
/// without requiring a per-keystroke IPC round-trip. Creates a session for
/// `relpath` if none exists; leaves the undo history untouched.
#[tauri::command]
pub async fn editor_sync_content(
    relpath: String,
    content: String,
    runtime: State<'_, KernelRuntime>,
) -> Result<serde_json::Value, String> {
    call_editor(
        runtime,
        "sync_content",
        serde_json::json!({ "relpath": relpath, "content": content }),
    )
    .await
}
