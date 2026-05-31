//! Core plugin wrapping [`crate::CommentStore`].
//!
//! Exposes the comment store over kernel IPC so the shell's
//! side-margin pane (BL-050 follow-up) can read/write threads via
//! `context.ipc_call("com.nexus.comments", ...)` without linking
//! this crate directly. Same pattern as `com.nexus.skills` /
//! `com.nexus.linkpreview`.
//!
//! # Handlers
//!
//! | Id | Command          | Args                                                               | Returns                            |
//! |---:|------------------|--------------------------------------------------------------------|------------------------------------|
//! | 1  | `list`           | `{ file_path }`                                                    | `Vec<Thread>`                      |
//! | 2  | `create_thread`  | `{ file_path, block_id, body, author? }`                           | `Thread`                           |
//! | 3  | `add_reply`      | `{ file_path, thread_id, body, author? }`                          | `Comment`                          |
//! | 4  | `set_resolved`   | `{ file_path, thread_id, resolved, author? }`                      | `Thread`                           |
//! | 5  | `delete_thread`  | `{ file_path, thread_id }`                                         | `{}`                               |
//! | 6  | `delete_comment` | `{ file_path, thread_id, comment_id }`                             | `{}`                               |
//! | 7  | `edit_comment`   | `{ file_path, thread_id, comment_id, body }`                       | `Comment`                          |
//!
//! Ids are append-only.

use std::path::Path;

use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::store::{CommentStore, CommentStoreError};

/// Reverse-DNS plugin id.
pub const PLUGIN_ID: &str = "com.nexus.comments";

/// `list` handler id.
pub const HANDLER_LIST: u32 = 1;
/// `create_thread` handler id.
pub const HANDLER_CREATE_THREAD: u32 = 2;
/// `add_reply` handler id.
pub const HANDLER_ADD_REPLY: u32 = 3;
/// `set_resolved` handler id.
pub const HANDLER_SET_RESOLVED: u32 = 4;
/// `delete_thread` handler id.
pub const HANDLER_DELETE_THREAD: u32 = 5;
/// `delete_comment` handler id.
pub const HANDLER_DELETE_COMMENT: u32 = 6;
/// `edit_comment` handler id.
pub const HANDLER_EDIT_COMMENT: u32 = 7;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::comments::register`.
/// Order matches the pre-SD-06 bootstrap registration.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("list", HANDLER_LIST),
    ("create_thread", HANDLER_CREATE_THREAD),
    ("add_reply", HANDLER_ADD_REPLY),
    ("set_resolved", HANDLER_SET_RESOLVED),
    ("delete_thread", HANDLER_DELETE_THREAD),
    ("delete_comment", HANDLER_DELETE_COMMENT),
    ("edit_comment", HANDLER_EDIT_COMMENT),
];

/// Stateless wrapper — every dispatch hits the JSON sidecar fresh.
pub struct CommentsCorePlugin {
    store: CommentStore,
}

impl CommentsCorePlugin {
    /// Construct a plugin rooted at the given forge directory.
    #[must_use]
    pub fn new(forge_root: &Path) -> Self {
        Self {
            store: CommentStore::new(forge_root),
        }
    }
}

impl CorePlugin for CommentsCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_LIST => dispatch_list(&self.store, args),
            HANDLER_CREATE_THREAD => dispatch_create_thread(&self.store, args),
            HANDLER_ADD_REPLY => dispatch_add_reply(&self.store, args),
            HANDLER_SET_RESOLVED => dispatch_set_resolved(&self.store, args),
            HANDLER_DELETE_THREAD => dispatch_delete_thread(&self.store, args),
            HANDLER_DELETE_COMMENT => dispatch_delete_comment(&self.store, args),
            HANDLER_EDIT_COMMENT => dispatch_edit_comment(&self.store, args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

// ── IPC arg types (audit P1-3 #113 — bumped to `pub` for schema gen) ────────

/// Args for `com.nexus.comments::list` (handler id `1`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct FilePathArg {
    /// Forge-relative path of the markdown file. Threads are
    /// addressed relative to a file rather than a uuid so the index
    /// survives a rename via comment-store migration.
    pub file_path: String,
}

/// Args for `com.nexus.comments::create_thread` (handler id `2`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct CreateThreadArgs {
    /// Forge-relative path of the markdown file.
    pub file_path: String,
    /// Stable block id the thread is anchored to. Caller must have
    /// ensured the block was stamped via `com.nexus.editor::stamp_block`
    /// first.
    pub block_id: Uuid,
    /// Body text of the first comment in the thread.
    pub body: String,
    /// Optional author display name.
    #[serde(default)]
    pub author: Option<String>,
}

/// Args for `com.nexus.comments::add_reply` (handler id `3`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AddReplyArgs {
    /// Forge-relative path of the markdown file.
    pub file_path: String,
    /// Thread to append to.
    pub thread_id: Uuid,
    /// Reply body.
    pub body: String,
    /// Optional author display name.
    #[serde(default)]
    pub author: Option<String>,
}

/// Args for `com.nexus.comments::set_resolved` (handler id `4`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SetResolvedArgs {
    /// Forge-relative path of the markdown file.
    pub file_path: String,
    /// Thread to mark.
    pub thread_id: Uuid,
    /// New resolved flag.
    pub resolved: bool,
    /// Author of the resolution flip (best-effort).
    #[serde(default)]
    pub author: Option<String>,
}

/// Args for `com.nexus.comments::delete_thread` (handler id `5`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct DeleteThreadArgs {
    /// Forge-relative path of the markdown file.
    pub file_path: String,
    /// Thread to delete.
    pub thread_id: Uuid,
}

/// Args for `com.nexus.comments::delete_comment` (handler id `6`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct DeleteCommentArgs {
    /// Forge-relative path of the markdown file.
    pub file_path: String,
    /// Thread containing the comment.
    pub thread_id: Uuid,
    /// Comment to delete.
    pub comment_id: Uuid,
}

/// Args for `com.nexus.comments::edit_comment` (handler id `7`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EditCommentArgs {
    /// Forge-relative path of the markdown file.
    pub file_path: String,
    /// Thread containing the comment.
    pub thread_id: Uuid,
    /// Comment to edit.
    pub comment_id: Uuid,
    /// New body text.
    pub body: String,
}

fn dispatch_list(
    store: &CommentStore,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: FilePathArg =
        serde_json::from_value(args.clone()).map_err(|e| exec_err(format!("list: {e}")))?;
    let threads = store
        .list_threads(&a.file_path)
        .map_err(|e| map_store_err(&e))?;
    serde_json::to_value(&threads).map_err(|e| exec_err(format!("list: serialize: {e}")))
}

/// Maximum byte size accepted for a comment body. Comment threads
/// persist to `<forge>/.forge/comments/<file>.json`; an unbounded
/// body causes the per-file JSON to balloon, slowing watcher reload
/// and bus replay. 64 KiB is far past any normal review comment and
/// well under the JSON-parser memory pressure threshold. See issue
/// #85.
const MAX_COMMENT_BODY_BYTES: usize = 64 * 1024;

fn check_body_size(body: &str, command: &str) -> Result<(), PluginError> {
    if body.len() > MAX_COMMENT_BODY_BYTES {
        return Err(exec_err(format!(
            "{command}: body is {} bytes; max is {MAX_COMMENT_BODY_BYTES} bytes",
            body.len()
        )));
    }
    Ok(())
}

fn dispatch_create_thread(
    store: &CommentStore,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: CreateThreadArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("create_thread: {e}")))?;
    check_body_size(&a.body, "create_thread")?;
    let thread = store
        .create_thread(&a.file_path, a.block_id, a.body, a.author)
        .map_err(|e| map_store_err(&e))?;
    serde_json::to_value(&thread).map_err(|e| exec_err(format!("create_thread: serialize: {e}")))
}

fn dispatch_add_reply(
    store: &CommentStore,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: AddReplyArgs =
        serde_json::from_value(args.clone()).map_err(|e| exec_err(format!("add_reply: {e}")))?;
    check_body_size(&a.body, "add_reply")?;
    let comment = store
        .add_reply(&a.file_path, a.thread_id, a.body, a.author)
        .map_err(|e| map_store_err(&e))?;
    serde_json::to_value(&comment).map_err(|e| exec_err(format!("add_reply: serialize: {e}")))
}

fn dispatch_set_resolved(
    store: &CommentStore,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: SetResolvedArgs =
        serde_json::from_value(args.clone()).map_err(|e| exec_err(format!("set_resolved: {e}")))?;
    let thread = store
        .set_resolved(&a.file_path, a.thread_id, a.resolved, a.author)
        .map_err(|e| map_store_err(&e))?;
    serde_json::to_value(&thread).map_err(|e| exec_err(format!("set_resolved: serialize: {e}")))
}

fn dispatch_delete_thread(
    store: &CommentStore,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: DeleteThreadArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("delete_thread: {e}")))?;
    store
        .delete_thread(&a.file_path, a.thread_id)
        .map_err(|e| map_store_err(&e))?;
    Ok(serde_json::json!({}))
}

fn dispatch_delete_comment(
    store: &CommentStore,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: DeleteCommentArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("delete_comment: {e}")))?;
    store
        .delete_comment(&a.file_path, a.thread_id, a.comment_id)
        .map_err(|e| map_store_err(&e))?;
    Ok(serde_json::json!({}))
}

fn dispatch_edit_comment(
    store: &CommentStore,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: EditCommentArgs =
        serde_json::from_value(args.clone()).map_err(|e| exec_err(format!("edit_comment: {e}")))?;
    check_body_size(&a.body, "edit_comment")?;
    let comment = store
        .edit_comment(&a.file_path, a.thread_id, a.comment_id, a.body)
        .map_err(|e| map_store_err(&e))?;
    serde_json::to_value(&comment).map_err(|e| exec_err(format!("edit_comment: serialize: {e}")))
}

fn map_store_err(err: &CommentStoreError) -> PluginError {
    exec_err(err.to_string())
}

nexus_plugins::define_dispatch_helpers!();

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn plugin() -> (TempDir, CommentsCorePlugin) {
        let dir = TempDir::new().unwrap();
        let p = CommentsCorePlugin::new(dir.path());
        (dir, p)
    }

    #[test]
    fn list_empty_returns_empty_array() {
        let (_d, mut p) = plugin();
        let out = p
            .dispatch(HANDLER_LIST, &json!({"file_path": "foo.md"}))
            .unwrap();
        assert_eq!(out, json!([]));
    }

    #[test]
    fn create_then_list_via_ipc() {
        let (_d, mut p) = plugin();
        let block_id = Uuid::new_v4();
        let created = p
            .dispatch(
                HANDLER_CREATE_THREAD,
                &json!({
                    "file_path": "foo.md",
                    "block_id": block_id,
                    "body": "hi",
                    "author": "alice",
                }),
            )
            .unwrap();
        let thread_id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["block_id"].as_str().unwrap(), block_id.to_string());

        let listed = p
            .dispatch(HANDLER_LIST, &json!({"file_path": "foo.md"}))
            .unwrap();
        let arr = listed.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"].as_str().unwrap(), thread_id);
    }

    #[test]
    fn add_reply_via_ipc() {
        let (_d, mut p) = plugin();
        let block_id = Uuid::new_v4();
        let created = p
            .dispatch(
                HANDLER_CREATE_THREAD,
                &json!({"file_path": "foo.md", "block_id": block_id, "body": "q?"}),
            )
            .unwrap();
        let tid = created["id"].as_str().unwrap();
        let reply = p
            .dispatch(
                HANDLER_ADD_REPLY,
                &json!({"file_path": "foo.md", "thread_id": tid, "body": "ans"}),
            )
            .unwrap();
        assert_eq!(reply["body"].as_str().unwrap(), "ans");

        let listed = p
            .dispatch(HANDLER_LIST, &json!({"file_path": "foo.md"}))
            .unwrap();
        assert_eq!(listed[0]["comments"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn set_resolved_and_unresolved() {
        let (_d, mut p) = plugin();
        let created = p
            .dispatch(
                HANDLER_CREATE_THREAD,
                &json!({"file_path": "foo.md", "block_id": Uuid::new_v4(), "body": "x"}),
            )
            .unwrap();
        let tid = created["id"].as_str().unwrap();

        let resolved = p
            .dispatch(
                HANDLER_SET_RESOLVED,
                &json!({"file_path": "foo.md", "thread_id": tid, "resolved": true, "author": "carol"}),
            )
            .unwrap();
        assert_eq!(resolved["resolved"], json!(true));
        assert_eq!(resolved["resolved_by"].as_str().unwrap(), "carol");

        let again = p
            .dispatch(
                HANDLER_SET_RESOLVED,
                &json!({"file_path": "foo.md", "thread_id": tid, "resolved": false}),
            )
            .unwrap();
        assert_eq!(again["resolved"], json!(false));
        assert!(again.get("resolved_by").is_none() || again["resolved_by"].is_null());
    }

    #[test]
    fn delete_thread_via_ipc() {
        let (_d, mut p) = plugin();
        let created = p
            .dispatch(
                HANDLER_CREATE_THREAD,
                &json!({"file_path": "foo.md", "block_id": Uuid::new_v4(), "body": "x"}),
            )
            .unwrap();
        let tid = created["id"].as_str().unwrap();
        p.dispatch(
            HANDLER_DELETE_THREAD,
            &json!({"file_path": "foo.md", "thread_id": tid}),
        )
        .unwrap();
        let listed = p
            .dispatch(HANDLER_LIST, &json!({"file_path": "foo.md"}))
            .unwrap();
        assert_eq!(listed, json!([]));
    }

    #[test]
    fn unknown_handler_errors() {
        let (_d, mut p) = plugin();
        let err = p.dispatch(99, &json!({})).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown handler id 99"), "got: {msg}");
    }

    #[test]
    fn edit_comment_via_ipc() {
        let (_d, mut p) = plugin();
        let created = p
            .dispatch(
                HANDLER_CREATE_THREAD,
                &json!({"file_path": "foo.md", "block_id": Uuid::new_v4(), "body": "old"}),
            )
            .unwrap();
        let tid = created["id"].as_str().unwrap();
        let cid = created["comments"][0]["id"].as_str().unwrap();
        let edited = p
            .dispatch(
                HANDLER_EDIT_COMMENT,
                &json!({"file_path": "foo.md", "thread_id": tid, "comment_id": cid, "body": "new"}),
            )
            .unwrap();
        assert_eq!(edited["body"].as_str().unwrap(), "new");
        assert!(edited["updated_at"].is_string());
    }

    #[test]
    fn invalid_path_surfaces_as_error() {
        let (_d, mut p) = plugin();
        let err = p
            .dispatch(HANDLER_LIST, &json!({"file_path": "/etc/passwd"}))
            .unwrap_err();
        assert!(err.to_string().contains("invalid file path"));
    }
}
