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
//! | 8  | `facts`  | `{ subject?, predicate?, … }` | Recall SPO entity facts          |
//! | 9  | `entities` | `{ limit? }`                | Distinct entities + fact counts  |
//! | 10 | `export` | `{}`                          | Dump every memory, oldest first  |
//! | 11 | `tags`   | `{ limit? }`                  | Distinct tags + memory counts    |
//! | 12 | `vitality_report` | `{ limit? }`         | Active memories ranked by vitality |
//! | 13 | `recall` | `{ query, limit? }`           | Hybrid FTS + vector recall (RRF)  |
//! | 14 | `vector_sync` | `{ limit? }`             | Backfill memory embeddings        |
//! | 15 | `sync`   | `{ hub_url, secret, node_id }` | Push/pull with a memory hub      |
//!
//! Handlers 13–15 are **async** and dispatch through
//! [`CorePlugin::dispatch_async`]: 13–14 make nested `ipc_call`s to
//! `com.nexus.ai::embed_text` and `com.nexus.storage`'s namespaced vector
//! store; 15 (`sync`) makes outbound HTTP to a `nexus-memory-hub`. The rest are
//! synchronous. Ids are append-only. The remaining `remind_me` parity commands
//! (capture/wiki/consolidate) land in later phases.

use std::path::Path;
use std::sync::Arc;

use nexus_kernel::KernelPluginContext;
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
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
/// `facts` handler id.
pub const HANDLER_FACTS: u32 = 8;
/// `entities` handler id.
pub const HANDLER_ENTITIES: u32 = 9;
/// `export` handler id.
pub const HANDLER_EXPORT: u32 = 10;
/// `tags` handler id.
pub const HANDLER_TAGS: u32 = 11;
/// `vitality_report` handler id.
pub const HANDLER_VITALITY_REPORT: u32 = 12;
/// `recall` handler id (async).
pub const HANDLER_RECALL: u32 = 13;
/// `vector_sync` handler id (async).
pub const HANDLER_VECTOR_SYNC: u32 = 14;
/// `sync` handler id (async; HTTP push/pull with a memory hub).
pub const HANDLER_SYNC: u32 = 15;

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
    ("facts", HANDLER_FACTS),
    ("entities", HANDLER_ENTITIES),
    ("export", HANDLER_EXPORT),
    ("tags", HANDLER_TAGS),
    ("vitality_report", HANDLER_VITALITY_REPORT),
    ("recall", HANDLER_RECALL),
    ("vector_sync", HANDLER_VECTOR_SYNC),
    ("sync", HANDLER_SYNC),
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
    /// Optional subject of an SPO entity fact (e.g. `ada`).
    #[serde(default)]
    pub subject: Option<String>,
    /// Optional predicate of an SPO entity fact (e.g. `writes`).
    #[serde(default)]
    pub predicate: Option<String>,
    /// Optional object of an SPO entity fact (e.g. `rust`).
    #[serde(default)]
    pub object: Option<String>,
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
    /// Optional category filter.
    #[serde(default)]
    pub category: Option<String>,
    /// Optional memory-type filter (`episodic` | `semantic` | `procedural` | `unclassified`).
    #[serde(default)]
    pub memory_type: Option<String>,
    /// Optional lifecycle-status filter (`active` | `archived` | `superseded`).
    #[serde(default)]
    pub status: Option<String>,
    /// Optional tag filter — matches memories whose tag list contains it.
    #[serde(default)]
    pub tag: Option<String>,
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
    /// New SPO subject, if changing.
    #[serde(default)]
    pub subject: Option<String>,
    /// New SPO predicate, if changing.
    #[serde(default)]
    pub predicate: Option<String>,
    /// New SPO object, if changing.
    #[serde(default)]
    pub object: Option<String>,
}

/// Args for `com.nexus.memory::facts` (handler id `8`). Recalls SPO entity
/// facts; each provided field narrows the result (omitted = any).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
pub struct FactsArgs {
    /// Optional subject filter.
    #[serde(default)]
    pub subject: Option<String>,
    /// Optional predicate filter.
    #[serde(default)]
    pub predicate: Option<String>,
    /// Optional object filter.
    #[serde(default)]
    pub object: Option<String>,
    /// Maximum rows to return (default 50).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Args for `com.nexus.memory::entities` (handler id `9`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
pub struct EntitiesArgs {
    /// Maximum entities to return (default 50).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Args for `com.nexus.memory::tags` (handler id `11`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
pub struct TagsArgs {
    /// Maximum tags to return (default 50).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Args for `com.nexus.memory::vitality_report` (handler id `12`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
pub struct VitalityReportArgs {
    /// Maximum memories to return (default 50).
    #[serde(default)]
    pub limit: Option<usize>,
}

// ── Plugin ───────────────────────────────────────────────────────────────────

/// Core plugin wrapping a [`MemoryDb`].
pub struct MemoryCorePlugin {
    db: MemoryDb,
    /// Plugin-facing kernel context, installed via [`CorePlugin::wire_context`].
    /// `Some` once bootstrap wires it; the async handlers (`recall`,
    /// `vector_sync`) use it to `ipc_call` the AI + storage plugins. `None`
    /// during early bootstrap / unit tests — `recall` then runs FTS-only.
    context: Option<Arc<KernelPluginContext>>,
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
        Ok(Self { db, context: None })
    }

    /// Construct directly over an existing [`MemoryDb`] (tests / embedding).
    #[must_use]
    pub fn with_db(db: MemoryDb) -> Self {
        Self { db, context: None }
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
        if a.subject.is_some() {
            m.subject = a.subject;
        }
        if a.predicate.is_some() {
            m.predicate = a.predicate;
        }
        if a.object.is_some() {
            m.object = a.object;
        }
        self.db.insert(&m).map_err(db_err)?;
        to_value(&m, "add")
    }

    fn get(&self, args: &Value) -> Result<Value, PluginError> {
        let a: IdArgs = parse_args(args, "get")?;
        // A `get` by id is a deliberate recall, so it records the access
        // (bumps access_count / accessed_at) — the ACT-R vitality input.
        match self.db.get_recording_access(parse_id(&a.id)?).map_err(db_err)? {
            Some(m) => to_value(&m, "get"),
            None => Err(exec_err(format!("no memory with id '{}'", a.id))),
        }
    }

    fn list(&self, args: &Value) -> Result<Value, PluginError> {
        let a: ListArgs = parse_args(args, "list")?;
        let mems = self
            .db
            .list_filtered(
                a.category.as_deref(),
                a.memory_type.as_deref(),
                a.status.as_deref(),
                a.tag.as_deref(),
                a.limit.unwrap_or(DEFAULT_LIST_LIMIT),
            )
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
        if a.subject.is_some() {
            m.subject = a.subject;
        }
        if a.predicate.is_some() {
            m.predicate = a.predicate;
        }
        if a.object.is_some() {
            m.object = a.object;
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
        to_value(&self.db.stats().map_err(db_err)?, "stats")
    }

    fn facts(&self, args: &Value) -> Result<Value, PluginError> {
        let a: FactsArgs = parse_args(args, "facts")?;
        let mems = self
            .db
            .list_facts(
                a.subject.as_deref(),
                a.predicate.as_deref(),
                a.object.as_deref(),
                a.limit.unwrap_or(DEFAULT_LIST_LIMIT),
            )
            .map_err(db_err)?;
        to_value(&mems, "facts")
    }

    fn entities(&self, args: &Value) -> Result<Value, PluginError> {
        let a: EntitiesArgs = parse_args(args, "entities")?;
        let ents = self
            .db
            .list_entities(a.limit.unwrap_or(DEFAULT_LIST_LIMIT))
            .map_err(db_err)?;
        to_value(&ents, "entities")
    }

    fn export(&self) -> Result<Value, PluginError> {
        to_value(&self.db.export_all().map_err(db_err)?, "export")
    }

    fn tags(&self, args: &Value) -> Result<Value, PluginError> {
        let a: TagsArgs = parse_args(args, "tags")?;
        let tags = self
            .db
            .list_tags(a.limit.unwrap_or(DEFAULT_LIST_LIMIT))
            .map_err(db_err)?;
        to_value(&tags, "tags")
    }

    fn vitality_report(&self, args: &Value) -> Result<Value, PluginError> {
        let a: VitalityReportArgs = parse_args(args, "vitality_report")?;
        let mems = self
            .db
            .vitality_report(a.limit.unwrap_or(DEFAULT_LIST_LIMIT))
            .map_err(db_err)?;
        to_value(&mems, "vitality_report")
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
            HANDLER_FACTS => self.facts(args),
            HANDLER_ENTITIES => self.entities(args),
            HANDLER_EXPORT => self.export(),
            HANDLER_TAGS => self.tags(args),
            HANDLER_VITALITY_REPORT => self.vitality_report(args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }

    fn dispatch_async(&mut self, handler_id: u32, args: &Value) -> Option<CorePluginFuture> {
        // The async handlers: recall/vector_sync `ipc_call` AI + storage;
        // sync makes outbound HTTP to a memory hub. Everything else falls
        // through to the synchronous `dispatch`.
        if handler_id != HANDLER_RECALL
            && handler_id != HANDLER_VECTOR_SYNC
            && handler_id != HANDLER_SYNC
        {
            return None;
        }
        let db = self.db.clone();
        let ctx = self.context.clone();
        let args = args.clone();
        Some(Box::pin(async move {
            let result = match handler_id {
                HANDLER_RECALL => crate::vector::recall(db, ctx, &args).await,
                HANDLER_VECTOR_SYNC => crate::vector::vector_sync(db, ctx, &args).await,
                HANDLER_SYNC => crate::sync::sync(db, &args).await,
                _ => unreachable!("guarded above"),
            };
            result.map_err(exec_err)
        }))
    }

    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        self.context = Some(ctx);
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
        assert_eq!(stats["by_category"][0]["key"], "ops");
        assert_eq!(stats["by_category"][0]["count"], 1);
        assert_eq!(stats["by_memory_type"][0]["key"], "semantic");
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
    fn get_records_access_each_call() {
        let mut p = plugin();
        let id = p
            .dispatch(HANDLER_ADD, &json!({ "content": "remember this" }))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let first = p.dispatch(HANDLER_GET, &json!({ "id": id })).unwrap();
        assert_eq!(first["access_count"], 1);
        assert!(first["accessed_at"].is_string());
        let second = p.dispatch(HANDLER_GET, &json!({ "id": id })).unwrap();
        assert_eq!(second["access_count"], 2);
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
    fn add_stores_spo_fact_and_facts_recalls_it() {
        let mut p = plugin();
        // A plain memory and two SPO facts.
        p.dispatch(HANDLER_ADD, &json!({ "content": "loose note" })).unwrap();
        let fact = p
            .dispatch(
                HANDLER_ADD,
                &json!({
                    "content": "Ada writes Rust",
                    "subject": "ada", "predicate": "writes", "object": "rust",
                }),
            )
            .unwrap();
        assert_eq!(fact["subject"], "ada");
        assert_eq!(fact["object"], "rust");
        p.dispatch(
            HANDLER_ADD,
            &json!({ "content": "Ada lives in London", "subject": "ada", "predicate": "lives_in", "object": "london" }),
        )
        .unwrap();

        // facts with no filter excludes the plain note.
        let all = p.dispatch(HANDLER_FACTS, &json!({})).unwrap();
        assert_eq!(all.as_array().unwrap().len(), 2);
        // Narrow by subject + object.
        let one = p
            .dispatch(HANDLER_FACTS, &json!({ "subject": "ada", "object": "rust" }))
            .unwrap();
        assert_eq!(one.as_array().unwrap().len(), 1);
        assert_eq!(one[0]["content"], "Ada writes Rust");
    }

    #[test]
    fn update_can_set_spo_fields() {
        let mut p = plugin();
        let id = p
            .dispatch(HANDLER_ADD, &json!({ "content": "Ada knows Lovelace" }))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        // Not a fact yet.
        assert_eq!(p.dispatch(HANDLER_FACTS, &json!({})).unwrap().as_array().unwrap().len(), 0);
        p.dispatch(
            HANDLER_UPDATE,
            &json!({ "id": id, "subject": "ada", "predicate": "knows", "object": "lovelace" }),
        )
        .unwrap();
        let facts = p.dispatch(HANDLER_FACTS, &json!({ "predicate": "knows" })).unwrap();
        assert_eq!(facts.as_array().unwrap().len(), 1);
        assert_eq!(facts[0]["subject"], "ada");
    }

    #[test]
    fn entities_lists_distinct_entities_with_counts() {
        let mut p = plugin();
        p.dispatch(HANDLER_ADD, &json!({ "content": "no entity here" })).unwrap();
        p.dispatch(
            HANDLER_ADD,
            &json!({ "content": "Ada writes Rust", "subject": "ada", "predicate": "writes", "object": "rust" }),
        )
        .unwrap();
        p.dispatch(
            HANDLER_ADD,
            &json!({ "content": "Ada writes Ada", "subject": "ada", "predicate": "writes", "object": "ada-lang" }),
        )
        .unwrap();

        let ents = p.dispatch(HANDLER_ENTITIES, &json!({})).unwrap();
        let arr = ents.as_array().unwrap();
        // ada(2, as subject twice) + rust(1) + ada-lang(1) = 3 distinct.
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["key"], "ada");
        assert_eq!(arr[0]["count"], 2);
    }

    #[test]
    fn export_returns_all_memories() {
        let mut p = plugin();
        p.dispatch(HANDLER_ADD, &json!({ "content": "one" })).unwrap();
        p.dispatch(HANDLER_ADD, &json!({ "content": "two" })).unwrap();
        let dump = p.dispatch(HANDLER_EXPORT, &json!({})).unwrap();
        let arr = dump.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // Full records: ids and content present, ready for re-import.
        assert!(arr.iter().all(|m| m["id"].is_string() && m["content"].is_string()));
    }

    #[test]
    fn list_filters_by_tag_and_tags_lists_facets() {
        let mut p = plugin();
        p.dispatch(HANDLER_ADD, &json!({ "content": "a", "tags": ["infra", "k8s"] })).unwrap();
        p.dispatch(HANDLER_ADD, &json!({ "content": "b", "tags": ["infra"] })).unwrap();
        p.dispatch(HANDLER_ADD, &json!({ "content": "c" })).unwrap();

        // list with a tag filter.
        let infra = p.dispatch(HANDLER_LIST, &json!({ "tag": "infra" })).unwrap();
        assert_eq!(infra.as_array().unwrap().len(), 2);
        let k8s = p.dispatch(HANDLER_LIST, &json!({ "tag": "k8s" })).unwrap();
        assert_eq!(k8s.as_array().unwrap().len(), 1);

        // tags facet, most-frequent first.
        let tags = p.dispatch(HANDLER_TAGS, &json!({})).unwrap();
        let arr = tags.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["key"], "infra");
        assert_eq!(arr[0]["count"], 2);
    }

    #[test]
    fn vitality_report_ranks_accessed_memories_first() {
        let mut p = plugin();
        // Two memories; recall one of them several times so it outranks.
        let hot = p.dispatch(HANDLER_ADD, &json!({ "content": "hot" })).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        p.dispatch(HANDLER_ADD, &json!({ "content": "cold" })).unwrap();
        for _ in 0..5 {
            p.dispatch(HANDLER_GET, &json!({ "id": hot })).unwrap();
        }
        let report = p.dispatch(HANDLER_VITALITY_REPORT, &json!({})).unwrap();
        let arr = report.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["content"], "hot"); // most-accessed ranks first
    }

    #[test]
    fn unknown_handler_errors() {
        let mut p = plugin();
        assert!(p.dispatch(9999, &json!({})).is_err());
    }
}
