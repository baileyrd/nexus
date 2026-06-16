//! Core plugin exposing the memory engine over kernel IPC (`com.nexus.memory`).
//!
//! Wraps a [`MemoryDb`] and dispatches CRUD + search so the agent loop, shell,
//! MCP server, and external clients can capture and recall memories through the
//! one `ipc_call` path — without linking `nexus-memory` directly.
//!
//! # Handlers
//!
//! | Id | Command  | Args                          | Purpose                          |
//! |---:|----------|-------------------------------|----------------------------------|
//! | 1  | `add`    | `{ content, category?, … }`   | Store a memory; returns it       |
//! | 2  | `get`    | `{ id }`                      | Fetch one (404 if missing)       |
//! | 3  | `list`   | `{ limit? }`                  | Recent memories, newest first    |
//! | 4  | `search` | `{ query, limit? }`           | Full-text search (`FTS5`)        |
//! | 5  | `update` | `{ id, content?, … }`         | Patch mutable fields             |
//! | 6  | `delete` | `{ id }`                      | Remove a memory                  |
//! | 7  | `stats`  | `{}`                          | Store statistics (count)         |
//!
//! Ids are append-only. Hybrid vector recall and the `remind_me` parity commands
//! (capture/entity/wiki/lifecycle) land in later phases.

use std::path::Path;

use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::db::{MemoryDb, MemoryDbError};
use crate::model::{Memory, MemoryStatus, MemoryType};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.memory";

/// `add` handler id.
pub const HANDLER_ADD: u32 = 1;
/// `get` handler id.
pub const HANDLER_GET: u32 = 2;
/// `list` handler id.
pub const HANDLER_LIST: u32 = 3;
/// `search` handler id.
pub const HANDLER_SEARCH: u32 = 4;
/// `update` handler id.
pub const HANDLER_UPDATE: u32 = 5;
/// `delete` handler id.
pub const HANDLER_DELETE: u32 = 6;
/// `stats` handler id.
pub const HANDLER_STATS: u32 = 7;

/// Single source of truth for `(command-name, handler-id)` pairs consumed by
/// the bootstrap registration. Order matches the handler-id numbering.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("add", HANDLER_ADD),
    ("get", HANDLER_GET),
    ("list", HANDLER_LIST),
    ("search", HANDLER_SEARCH),
    ("update", HANDLER_UPDATE),
    ("delete", HANDLER_DELETE),
    ("stats", HANDLER_STATS),
];

/// Default number of rows returned by `list` when no limit is given.
const DEFAULT_LIST_LIMIT: usize = 50;
/// Default number of rows returned by `search` when no limit is given.
const DEFAULT_SEARCH_LIMIT: usize = 20;

// ── IPC arg types ────────────────────────────────────────────────────────────

/// Args for `com.nexus.memory::add` (handler id `1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
pub struct AddArgs {
    /// Memory text to store.
    pub content: String,
    /// Optional category (defaults to `general`).
    #[serde(default)]
    pub category: Option<String>,
    /// Optional tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional source label (defaults to `manual`).
    #[serde(default)]
    pub source: Option<String>,
    /// Optional cognitive type: `episodic` | `semantic` | `procedural` | `unclassified`.
    #[serde(default)]
    pub memory_type: Option<String>,
    /// Optional originating client/provider (e.g. `claude`, `openai`, `ollama`).
    #[serde(default)]
    pub client: Option<String>,
}

/// Args for handlers that address a single memory by id (`get`, `delete`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
pub struct IdArgs {
    /// The memory id (UUID string).
    pub id: String,
}

/// Args for `com.nexus.memory::list` (handler id `3`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
pub struct ListArgs {
    /// Maximum rows to return (default 50).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Args for `com.nexus.memory::search` (handler id `4`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    /// FTS5 query string.
    pub query: String,
    /// Maximum rows to return (default 20).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Args for `com.nexus.memory::update` (handler id `5`). Only the provided
/// fields are changed; omitted fields keep their stored value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
pub struct UpdateArgs {
    /// The memory id (UUID string) to update.
    pub id: String,
    /// New content, if changing.
    #[serde(default)]
    pub content: Option<String>,
    /// New category, if changing.
    #[serde(default)]
    pub category: Option<String>,
    /// Replacement tag list, if changing.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// New lifecycle status (`active` | `archived` | `superseded`), if changing.
    #[serde(default)]
    pub status: Option<String>,
}

// ── Plugin ───────────────────────────────────────────────────────────────────

/// Core plugin wrapping a [`MemoryDb`].
pub struct MemoryCorePlugin {
    db: MemoryDb,
}

impl MemoryCorePlugin {
    /// Open (creating if needed) the forge's memory database at
    /// `<forge_root>/.forge/memory/memory.db`.
    ///
    /// # Errors
    /// Returns an error if the memory directory cannot be created or the
    /// database cannot be opened.
    pub fn open(forge_root: &Path) -> Result<Self, MemoryDbError> {
        let dir = forge_root.join(".forge").join("memory");
        std::fs::create_dir_all(&dir)?;
        let db = MemoryDb::open(&dir.join("memory.db"))?;
        Ok(Self { db })
    }

    /// Construct directly over an existing [`MemoryDb`] (tests / embedding).
    #[must_use]
    pub fn with_db(db: MemoryDb) -> Self {
        Self { db }
    }

    fn add(&self, args: &Value) -> Result<Value, PluginError> {
        let a: AddArgs = parse_args(args, "add")?;
        let mut m = Memory::new(a.content);
        if let Some(c) = a.category {
            m.category = c;
        }
        if !a.tags.is_empty() {
            m.tags = a.tags;
        }
        if let Some(s) = a.source {
            m.source = s;
        }
        if let Some(t) = a.memory_type {
            m.memory_type = MemoryType::from_db(&t);
        }
        if let Some(c) = a.client {
            m.client = c;
        }
        self.db.insert(&m).map_err(db_err)?;
        to_value(&m, "add")
    }

    fn get(&self, args: &Value) -> Result<Value, PluginError> {
        let a: IdArgs = parse_args(args, "get")?;
        match self.db.get(parse_id(&a.id)?).map_err(db_err)? {
            Some(m) => to_value(&m, "get"),
            None => Err(exec_err(format!("no memory with id '{}'", a.id))),
        }
    }

    fn list(&self, args: &Value) -> Result<Value, PluginError> {
        let a: ListArgs = parse_args(args, "list")?;
        let mems = self
            .db
            .list(a.limit.unwrap_or(DEFAULT_LIST_LIMIT))
            .map_err(db_err)?;
        to_value(&mems, "list")
    }

    fn search(&self, args: &Value) -> Result<Value, PluginError> {
        let a: SearchArgs = parse_args(args, "search")?;
        let mems = self
            .db
            .search(&a.query, a.limit.unwrap_or(DEFAULT_SEARCH_LIMIT))
            .map_err(db_err)?;
        to_value(&mems, "search")
    }

    fn update(&self, args: &Value) -> Result<Value, PluginError> {
        let a: UpdateArgs = parse_args(args, "update")?;
        let id = parse_id(&a.id)?;
        let mut m = self
            .db
            .get(id)
            .map_err(db_err)?
            .ok_or_else(|| exec_err(format!("no memory with id '{}'", a.id)))?;
        if let Some(c) = a.content {
            m.content = c;
        }
        if let Some(c) = a.category {
            m.category = c;
        }
        if let Some(t) = a.tags {
            m.tags = t;
        }
        if let Some(s) = a.status {
            m.status = MemoryStatus::from_db(&s);
        }
        let updated = self.db.update(&m).map_err(db_err)?;
        Ok(json!({ "updated": updated }))
    }

    fn delete(&self, args: &Value) -> Result<Value, PluginError> {
        let a: IdArgs = parse_args(args, "delete")?;
        let deleted = self.db.delete(parse_id(&a.id)?).map_err(db_err)?;
        Ok(json!({ "deleted": deleted }))
    }

    fn stats(&self) -> Result<Value, PluginError> {
        Ok(json!({ "count": self.db.count().map_err(db_err)? }))
    }
}

impl CorePlugin for MemoryCorePlugin {
    fn dispatch(&mut self, handler_id: u32, args: &Value) -> Result<Value, PluginError> {
        match handler_id {
            HANDLER_ADD => self.add(args),
            HANDLER_GET => self.get(args),
            HANDLER_LIST => self.list(args),
            HANDLER_SEARCH => self.search(args),
            HANDLER_UPDATE => self.update(args),
            HANDLER_DELETE => self.delete(args),
            HANDLER_STATS => self.stats(),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

nexus_plugins::define_dispatch_helpers!();

/// Map a database-layer error onto the plugin-facing error type.
// Owned by value so it composes ergonomically as `.map_err(db_err)`.
#[allow(clippy::needless_pass_by_value)]
fn db_err(e: MemoryDbError) -> PluginError {
    exec_err(format!("memory db: {e}"))
}

/// Parse a UUID string argument, surfacing a clear error on malformed input.
fn parse_id(s: &str) -> Result<Uuid, PluginError> {
    Uuid::parse_str(s).map_err(|e| exec_err(format!("invalid memory id '{s}': {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin() -> MemoryCorePlugin {
        MemoryCorePlugin::with_db(MemoryDb::open_in_memory().unwrap())
    }

    #[test]
    fn add_then_get_search_and_stats() {
        let mut p = plugin();
        let added = p
            .dispatch(
                HANDLER_ADD,
                &json!({ "content": "deploy runs on Kubernetes", "category": "ops", "memory_type": "semantic" }),
            )
            .unwrap();
        assert_eq!(added["memory_type"], "semantic");
        assert_eq!(added["category"], "ops");
        let id = added["id"].as_str().unwrap().to_string();

        let got = p.dispatch(HANDLER_GET, &json!({ "id": id })).unwrap();
        assert_eq!(got["content"], "deploy runs on Kubernetes");

        let hits = p.dispatch(HANDLER_SEARCH, &json!({ "query": "kubernetes" })).unwrap();
        assert_eq!(hits.as_array().unwrap().len(), 1);

        let stats = p.dispatch(HANDLER_STATS, &json!({})).unwrap();
        assert_eq!(stats["count"], 1);
    }

    #[test]
    fn update_patches_only_given_fields() {
        let mut p = plugin();
        let id = p
            .dispatch(HANDLER_ADD, &json!({ "content": "old", "category": "a" }))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let upd = p
            .dispatch(HANDLER_UPDATE, &json!({ "id": id, "content": "new", "status": "archived" }))
            .unwrap();
        assert_eq!(upd["updated"], true);
        let got = p.dispatch(HANDLER_GET, &json!({ "id": id })).unwrap();
        assert_eq!(got["content"], "new");
        assert_eq!(got["status"], "archived");
        assert_eq!(got["category"], "a"); // untouched
    }

    #[test]
    fn delete_removes_memory() {
        let mut p = plugin();
        let id = p
            .dispatch(HANDLER_ADD, &json!({ "content": "ephemeral" }))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let del = p.dispatch(HANDLER_DELETE, &json!({ "id": id })).unwrap();
        assert_eq!(del["deleted"], true);
        assert_eq!(p.dispatch(HANDLER_STATS, &json!({})).unwrap()["count"], 0);
    }

    #[test]
    fn get_unknown_id_errors() {
        let mut p = plugin();
        let err = p
            .dispatch(HANDLER_GET, &json!({ "id": Uuid::now_v7().to_string() }))
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => assert!(reason.contains("no memory")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn invalid_id_errors() {
        let mut p = plugin();
        let err = p.dispatch(HANDLER_GET, &json!({ "id": "not-a-uuid" })).unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => assert!(reason.contains("invalid memory id")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn unknown_handler_errors() {
        let mut p = plugin();
        assert!(p.dispatch(9999, &json!({})).is_err());
    }
}
