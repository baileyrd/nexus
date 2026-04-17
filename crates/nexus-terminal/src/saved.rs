//! Saved commands + ad-hoc promotion (PRD-09 §10.2, §13, §15).
//!
//! # Role
//!
//! The saved-command sidebar (§14.1) reads from `procmgr_commands`.
//! This module owns that table: CRUD, ordering, and the "promote an
//! ad-hoc run into a saved command" flow from §10.2 that ties the
//! history store to the sidebar store.
//!
//! # Microkernel fit
//!
//! Plain library; no kernel bus. A future `com.nexus.terminal` core
//! plugin will surface `list / create / update / delete / reorder /
//! promote` as dispatch handlers over a `Mutex<SqliteSavedCommandStore>`
//! — this module doesn't know about plugin IPC.
//!
//! # What this is NOT
//!
//! - Execution. [`SavedCommand`] is pure configuration; spawning it is
//!   the [`crate::procmgr`] / terminal-server layer's job. Keeping this
//!   module schema-only means the saved-command list can be edited
//!   from a UI that doesn't even have a terminal attached.
//! - Env-var resolution. The `env_vars` / `env_file` fields round-trip
//!   as-is; [`crate::env::resolve_env`] reads them when a caller
//!   actually spawns.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::adhoc::SqliteAdHocStore;
use crate::error::TerminalError;
use crate::persist::unix_now;

/// Default icon tag written when the caller doesn't pick one, matching
/// the PRD-09 §13 schema default.
pub const DEFAULT_ICON: &str = "terminal";
/// Default auto-restart backoff before any exponential escalation,
/// matching PRD-09 §13.
pub const DEFAULT_AUTO_RESTART_DELAY_MS: u64 = 2_000;

/// One row of `procmgr_commands` — the saved-sidebar record a user
/// created through "Save as command" or the editor dialog (§15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedCommand {
    /// URL-safe primary key.
    pub slug: String,
    /// Human-readable label.
    pub name: String,
    /// Absolute shell path (`/bin/bash`, `cmd.exe`, `pwsh.exe`, …).
    pub shell: String,
    /// Full command (may include `&&` / `||` / `;`).
    pub shell_cmd: String,
    /// Working directory.
    pub working_dir: Option<String>,
    /// Per-command env vars (layer 1 in [`crate::env::resolve_env`]).
    pub env_vars: HashMap<String, String>,
    /// Optional `.env` file override (layer 2).
    pub env_file: Option<String>,
    /// Icon tag the sidebar renders.
    pub icon: String,
    /// Whether crashed processes auto-restart (§4.3).
    pub auto_restart: bool,
    /// First-restart delay in ms; later restarts back off on top of this.
    pub auto_restart_delay_ms: u64,
    /// Hard memory cap (MB) before the process is killed (§7.3).
    pub memory_limit_mb: Option<u32>,
    /// Drag-reorder index; lower sorts earlier.
    pub sidebar_order: Option<i32>,
    /// Ordered pre-command chain (§4.4).
    pub pre_commands: Vec<String>,
    /// Unix-seconds creation time.
    pub created_at: i64,
    /// Unix-seconds last-updated time.
    pub updated_at: i64,
}

impl SavedCommand {
    /// Build a baseline saved command — minimal fields, standard
    /// defaults. Callers layer domain-specific fields on top.
    #[must_use]
    pub fn new(
        slug: impl Into<String>,
        name: impl Into<String>,
        shell: impl Into<String>,
        shell_cmd: impl Into<String>,
    ) -> Self {
        let now = unix_now();
        Self {
            slug: slug.into(),
            name: name.into(),
            shell: shell.into(),
            shell_cmd: shell_cmd.into(),
            working_dir: None,
            env_vars: HashMap::new(),
            env_file: None,
            icon: DEFAULT_ICON.into(),
            auto_restart: false,
            auto_restart_delay_ms: DEFAULT_AUTO_RESTART_DELAY_MS,
            memory_limit_mb: None,
            sidebar_order: None,
            pre_commands: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// SQLite-backed `procmgr_commands` store.
pub struct SqliteSavedCommandStore {
    conn: Connection,
}

impl SqliteSavedCommandStore {
    /// Open or create the store at `db_path`.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, TerminalError> {
        let conn = Connection::open(db_path.as_ref())
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory store for tests.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn in_memory() -> Result<Self, TerminalError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    fn migrate(conn: &Connection) -> Result<(), TerminalError> {
        conn.execute_batch(
            r"
            CREATE TABLE IF NOT EXISTS procmgr_commands (
                slug TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                shell TEXT NOT NULL,
                shell_cmd TEXT NOT NULL,
                working_dir TEXT,
                env_vars TEXT NOT NULL DEFAULT '{}',
                env_file TEXT,
                icon TEXT NOT NULL DEFAULT 'terminal',
                auto_restart INTEGER NOT NULL DEFAULT 0,
                auto_restart_delay_ms INTEGER NOT NULL DEFAULT 2000,
                memory_limit_mb INTEGER,
                sidebar_order INTEGER,
                pre_commands TEXT NOT NULL DEFAULT '[]',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_procmgr_commands_sidebar_order
                ON procmgr_commands(sidebar_order);
            ",
        )
        .map_err(|e| TerminalError::Persist(e.to_string()))
    }

    /// Insert a fresh row. Fails if the slug already exists.
    ///
    /// # Errors
    /// - [`TerminalError::Persist`] on duplicate slug / SQLite failures.
    pub fn create(&self, cmd: &SavedCommand) -> Result<(), TerminalError> {
        self.conn
            .execute(
                "INSERT INTO procmgr_commands (
                    slug, name, shell, shell_cmd, working_dir,
                    env_vars, env_file, icon,
                    auto_restart, auto_restart_delay_ms, memory_limit_mb,
                    sidebar_order, pre_commands, created_at, updated_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
                )",
                params![
                    cmd.slug,
                    cmd.name,
                    cmd.shell,
                    cmd.shell_cmd,
                    cmd.working_dir,
                    serde_json::to_string(&cmd.env_vars)
                        .map_err(|e| TerminalError::Persist(e.to_string()))?,
                    cmd.env_file,
                    cmd.icon,
                    i64::from(cmd.auto_restart),
                    i64::try_from(cmd.auto_restart_delay_ms).unwrap_or(i64::MAX),
                    cmd.memory_limit_mb.map(i64::from),
                    cmd.sidebar_order,
                    serde_json::to_string(&cmd.pre_commands)
                        .map_err(|e| TerminalError::Persist(e.to_string()))?,
                    cmd.created_at,
                    cmd.updated_at,
                ],
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Ok(())
    }

    /// Overwrite every field except `slug` and `created_at`. `updated_at`
    /// is bumped to now. Unknown slug is a persist error.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn update(&self, cmd: &SavedCommand) -> Result<(), TerminalError> {
        let changed = self
            .conn
            .execute(
                "UPDATE procmgr_commands SET
                    name = ?1,
                    shell = ?2,
                    shell_cmd = ?3,
                    working_dir = ?4,
                    env_vars = ?5,
                    env_file = ?6,
                    icon = ?7,
                    auto_restart = ?8,
                    auto_restart_delay_ms = ?9,
                    memory_limit_mb = ?10,
                    sidebar_order = ?11,
                    pre_commands = ?12,
                    updated_at = ?13
                 WHERE slug = ?14",
                params![
                    cmd.name,
                    cmd.shell,
                    cmd.shell_cmd,
                    cmd.working_dir,
                    serde_json::to_string(&cmd.env_vars)
                        .map_err(|e| TerminalError::Persist(e.to_string()))?,
                    cmd.env_file,
                    cmd.icon,
                    i64::from(cmd.auto_restart),
                    i64::try_from(cmd.auto_restart_delay_ms).unwrap_or(i64::MAX),
                    cmd.memory_limit_mb.map(i64::from),
                    cmd.sidebar_order,
                    serde_json::to_string(&cmd.pre_commands)
                        .map_err(|e| TerminalError::Persist(e.to_string()))?,
                    unix_now(),
                    cmd.slug,
                ],
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        if changed == 0 {
            return Err(TerminalError::Persist(format!(
                "no saved command with slug '{}'",
                cmd.slug
            )));
        }
        Ok(())
    }

    /// Load a saved command by slug, or `None` if missing.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn get(&self, slug: &str) -> Result<Option<SavedCommand>, TerminalError> {
        self.conn
            .query_row(
                "SELECT slug, name, shell, shell_cmd, working_dir,
                        env_vars, env_file, icon,
                        auto_restart, auto_restart_delay_ms, memory_limit_mb,
                        sidebar_order, pre_commands, created_at, updated_at
                 FROM procmgr_commands WHERE slug = ?1",
                params![slug],
                row_to_saved,
            )
            .optional()
            .map_err(|e| TerminalError::Persist(e.to_string()))?
            .transpose()
    }

    /// List every saved command, ordered by `sidebar_order` (nulls last)
    /// then by `name`. This is the shape the sidebar reads directly.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn list(&self) -> Result<Vec<SavedCommand>, TerminalError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT slug, name, shell, shell_cmd, working_dir,
                        env_vars, env_file, icon,
                        auto_restart, auto_restart_delay_ms, memory_limit_mb,
                        sidebar_order, pre_commands, created_at, updated_at
                 FROM procmgr_commands
                 ORDER BY
                    CASE WHEN sidebar_order IS NULL THEN 1 ELSE 0 END,
                    sidebar_order ASC,
                    name ASC",
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let rows = stmt
            .query_map([], row_to_saved)
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            let row = row.map_err(|e| TerminalError::Persist(e.to_string()))?;
            out.push(row?);
        }
        Ok(out)
    }

    /// Update just the `sidebar_order` field and bump `updated_at`.
    /// `None` drops the slug back to "unordered" bucket.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn reorder(&self, slug: &str, sidebar_order: Option<i32>) -> Result<(), TerminalError> {
        let changed = self
            .conn
            .execute(
                "UPDATE procmgr_commands SET sidebar_order = ?1, updated_at = ?2
                 WHERE slug = ?3",
                params![sidebar_order, unix_now(), slug],
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        if changed == 0 {
            return Err(TerminalError::Persist(format!(
                "no saved command with slug '{slug}'",
            )));
        }
        Ok(())
    }

    /// Delete a saved command. Silent no-op when the slug is unknown
    /// — destructive idempotence keeps UI flows simple.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn delete(&self, slug: &str) -> Result<(), TerminalError> {
        self.conn
            .execute(
                "DELETE FROM procmgr_commands WHERE slug = ?1",
                params![slug],
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Ok(())
    }
}

fn row_to_saved(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<SavedCommand, TerminalError>> {
    // We can't return `TerminalError` directly from `query_row`'s
    // closure, so thread it through `Result<SavedCommand, TerminalError>`
    // inside the rusqlite result. Callers above unwrap both layers.
    let env_vars_raw: String = row.get(5)?;
    let pre_commands_raw: String = row.get(12)?;
    let env_vars = match serde_json::from_str::<HashMap<String, String>>(&env_vars_raw) {
        Ok(m) => m,
        Err(e) => {
            return Ok(Err(TerminalError::Persist(format!(
                "parse env_vars json: {e}",
            ))));
        }
    };
    let pre_commands = match serde_json::from_str::<Vec<String>>(&pre_commands_raw) {
        Ok(v) => v,
        Err(e) => {
            return Ok(Err(TerminalError::Persist(format!(
                "parse pre_commands json: {e}",
            ))));
        }
    };
    let auto_restart_delay_ms: i64 = row.get(9)?;
    let memory_limit: Option<i64> = row.get(10)?;
    Ok(Ok(SavedCommand {
        slug: row.get(0)?,
        name: row.get(1)?,
        shell: row.get(2)?,
        shell_cmd: row.get(3)?,
        working_dir: row.get(4)?,
        env_vars,
        env_file: row.get(6)?,
        icon: row.get(7)?,
        auto_restart: row.get::<_, i64>(8)? != 0,
        auto_restart_delay_ms: u64::try_from(auto_restart_delay_ms).unwrap_or(0),
        memory_limit_mb: memory_limit.and_then(|v| u32::try_from(v).ok()),
        sidebar_order: row.get(11)?,
        pre_commands,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    }))
}

/// Slugify `name` into a URL-safe, lowercased identifier. Runs of
/// non-alphanumeric characters collapse into a single `_`; leading and
/// trailing `_` are trimmed.
///
/// Returns `fallback` when the input has no alphanumerics at all (e.g.
/// `"!!!"`) — callers usually pass the adhoc record's id, guaranteed
/// non-empty.
#[must_use]
pub fn slugify(name: &str, fallback: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_underscore = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_was_underscore = false;
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

/// Options for [`promote_adhoc_to_saved`] — the §10.2 "Save as Command"
/// flow. Only `name` is required; everything else falls through to
/// [`SavedCommand`] defaults or the originating ad-hoc row.
#[derive(Debug, Clone, Default)]
pub struct PromoteOptions {
    /// Optional explicit slug. When `None`, derive from `name` via
    /// [`slugify`]; the adhoc id is the fallback when `name` has no
    /// alphanumerics.
    pub slug: Option<String>,
    /// Icon tag. Defaults to [`DEFAULT_ICON`].
    pub icon: Option<String>,
    /// Shell to associate with the saved command. When `None`, use
    /// `/bin/sh` on Unix and `cmd.exe` on Windows — the caller almost
    /// always wants to override this, so we keep the default deliberately
    /// boring.
    pub shell: Option<String>,
}

/// Promote an ad-hoc execution into a first-class [`SavedCommand`]
/// (PRD-09 §10.2). Reads the adhoc row from `adhoc`, constructs a
/// saved command with the supplied options + defaults, and inserts it
/// into `saved`.
///
/// Returns the new [`SavedCommand`] (already persisted) so the caller
/// can render it in the sidebar without a round-trip.
///
/// # Errors
/// - [`TerminalError::Persist`] if the adhoc id is unknown or if
///   either SQLite call fails (e.g. slug collision).
pub fn promote_adhoc_to_saved(
    adhoc: &SqliteAdHocStore,
    saved: &SqliteSavedCommandStore,
    adhoc_id: &str,
    name: impl Into<String>,
    options: PromoteOptions,
) -> Result<SavedCommand, TerminalError> {
    let record = adhoc
        .get(adhoc_id)?
        .ok_or_else(|| TerminalError::Persist(format!("no adhoc row with id '{adhoc_id}'")))?;

    let name = name.into();
    let slug = options.slug.unwrap_or_else(|| slugify(&name, adhoc_id));
    let shell = options.shell.unwrap_or_else(default_shell_for_platform);
    let icon = options.icon.unwrap_or_else(|| DEFAULT_ICON.to_string());

    let mut cmd = SavedCommand::new(slug, &name, shell, record.command.clone());
    cmd.working_dir = record.working_dir;
    cmd.icon = icon;

    saved.create(&cmd)?;
    Ok(cmd)
}

#[cfg(unix)]
fn default_shell_for_platform() -> String {
    "/bin/sh".into()
}

#[cfg(not(unix))]
fn default_shell_for_platform() -> String {
    "cmd.exe".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adhoc::SqliteAdHocStore;

    fn cmd(slug: &str, name: &str) -> SavedCommand {
        SavedCommand::new(slug, name, "/bin/sh", "echo hi")
    }

    #[test]
    fn slugify_lowercases_and_collapses_non_alphanumerics() {
        assert_eq!(slugify("Build & Deploy", "fallback"), "build_deploy");
        assert_eq!(slugify("  npm run test  ", "f"), "npm_run_test");
        assert_eq!(slugify("dev-server:3000", "f"), "dev_server_3000");
    }

    #[test]
    fn slugify_uses_fallback_when_input_has_no_alphanumerics() {
        assert_eq!(slugify("!!!", "orig-id"), "orig-id");
        assert_eq!(slugify("", "backup"), "backup");
    }

    #[test]
    fn create_and_get_roundtrips_every_field() {
        let store = SqliteSavedCommandStore::in_memory().expect("open");
        let mut c = cmd("build", "Build");
        c.working_dir = Some("/repo".into());
        c.env_vars.insert("DEBUG".into(), "1".into());
        c.env_file = Some(".env.local".into());
        c.icon = "gear".into();
        c.auto_restart = true;
        c.auto_restart_delay_ms = 5_000;
        c.memory_limit_mb = Some(512);
        c.sidebar_order = Some(7);
        c.pre_commands = vec!["cd /repo".into(), "source venv/bin/activate".into()];
        store.create(&c).expect("create");

        let got = store.get("build").expect("get").expect("present");
        assert_eq!(got, c);
    }

    #[test]
    fn create_rejects_duplicate_slug() {
        let store = SqliteSavedCommandStore::in_memory().expect("open");
        store.create(&cmd("dup", "A")).expect("first");
        let err = store.create(&cmd("dup", "B")).unwrap_err();
        assert!(matches!(err, TerminalError::Persist(_)));
    }

    #[test]
    fn update_refreshes_fields_and_updated_at_but_preserves_created_at() {
        let store = SqliteSavedCommandStore::in_memory().expect("open");
        let mut c = cmd("x", "original");
        c.created_at = 1_000;
        c.updated_at = 1_000;
        store.create(&c).expect("create");
        std::thread::sleep(std::time::Duration::from_secs(1));
        c.name = "renamed".into();
        c.icon = "bug".into();
        store.update(&c).expect("update");
        let got = store.get("x").expect("get").expect("present");
        assert_eq!(got.name, "renamed");
        assert_eq!(got.icon, "bug");
        assert_eq!(got.created_at, 1_000, "created_at must be stable");
        assert!(got.updated_at > 1_000, "updated_at must advance");
    }

    #[test]
    fn update_unknown_slug_errors() {
        let store = SqliteSavedCommandStore::in_memory().expect("open");
        let err = store.update(&cmd("ghost", "nope")).unwrap_err();
        assert!(matches!(err, TerminalError::Persist(_)));
    }

    #[test]
    fn list_orders_by_sidebar_order_with_nulls_last() {
        let store = SqliteSavedCommandStore::in_memory().expect("open");
        let mut a = cmd("a", "alpha");
        a.sidebar_order = Some(2);
        let mut b = cmd("b", "bravo");
        b.sidebar_order = Some(1);
        let c = cmd("c", "charlie"); // None
        store.create(&c).expect("c");
        store.create(&a).expect("a");
        store.create(&b).expect("b");
        let list = store.list().expect("list");
        let slugs: Vec<&str> = list.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(slugs, vec!["b", "a", "c"]);
    }

    #[test]
    fn reorder_updates_only_sidebar_order_and_updated_at() {
        let store = SqliteSavedCommandStore::in_memory().expect("open");
        let mut c = cmd("x", "original");
        c.sidebar_order = Some(0);
        store.create(&c).expect("create");
        store.reorder("x", Some(5)).expect("reorder");
        let got = store.get("x").expect("get").expect("present");
        assert_eq!(got.sidebar_order, Some(5));
        assert_eq!(got.name, "original");
        // Clear back to unordered.
        store.reorder("x", None).expect("reorder none");
        assert_eq!(
            store.get("x").expect("get").expect("present").sidebar_order,
            None,
        );
    }

    #[test]
    fn delete_is_idempotent() {
        let store = SqliteSavedCommandStore::in_memory().expect("open");
        store.create(&cmd("x", "x")).expect("create");
        store.delete("x").expect("first");
        store.delete("x").expect("second — no-op"); // ok
        assert!(store.get("x").expect("get").is_none());
    }

    #[test]
    fn promote_adhoc_to_saved_copies_command_and_cwd() {
        let adhoc = SqliteAdHocStore::in_memory().expect("open adhoc");
        let saved = SqliteSavedCommandStore::in_memory().expect("open saved");
        let id = adhoc
            .record("npm run build", Some("/repo"), Some(0), 1500)
            .expect("record");
        let out = promote_adhoc_to_saved(
            &adhoc,
            &saved,
            &id,
            "Build",
            PromoteOptions {
                icon: Some("package".into()),
                shell: Some("/bin/bash".into()),
                ..Default::default()
            },
        )
        .expect("promote");
        assert_eq!(out.slug, "build");
        assert_eq!(out.shell_cmd, "npm run build");
        assert_eq!(out.working_dir.as_deref(), Some("/repo"));
        assert_eq!(out.icon, "package");
        assert_eq!(out.shell, "/bin/bash");
        // And it's actually in the saved store:
        let from_db = saved.get("build").expect("get").expect("present");
        assert_eq!(from_db, out);
    }

    #[test]
    fn promote_adhoc_with_explicit_slug_bypasses_slugify() {
        let adhoc = SqliteAdHocStore::in_memory().expect("adhoc");
        let saved = SqliteSavedCommandStore::in_memory().expect("saved");
        let id = adhoc.record("ls", None, Some(0), 10).expect("record");
        let out = promote_adhoc_to_saved(
            &adhoc,
            &saved,
            &id,
            "Whatever",
            PromoteOptions {
                slug: Some("custom-slug".into()),
                ..Default::default()
            },
        )
        .expect("promote");
        assert_eq!(out.slug, "custom-slug");
    }

    #[test]
    fn promote_with_unknown_adhoc_id_errors() {
        let adhoc = SqliteAdHocStore::in_memory().expect("adhoc");
        let saved = SqliteSavedCommandStore::in_memory().expect("saved");
        let err = promote_adhoc_to_saved(
            &adhoc,
            &saved,
            "ghost",
            "Name",
            PromoteOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(err, TerminalError::Persist(_)));
    }

    #[test]
    fn promote_twice_with_same_name_fails_the_second_time() {
        // Because slugify is deterministic, two promotes with the same
        // name collide on slug. UI should surface this and prompt for
        // a new slug.
        let adhoc = SqliteAdHocStore::in_memory().expect("adhoc");
        let saved = SqliteSavedCommandStore::in_memory().expect("saved");
        let id_a = adhoc.record("ls /a", None, Some(0), 10).expect("a");
        let id_b = adhoc.record("ls /b", None, Some(0), 10).expect("b");
        promote_adhoc_to_saved(&adhoc, &saved, &id_a, "Build", PromoteOptions::default())
            .expect("first");
        let err = promote_adhoc_to_saved(
            &adhoc,
            &saved,
            &id_b,
            "Build",
            PromoteOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(err, TerminalError::Persist(_)));
    }
}
