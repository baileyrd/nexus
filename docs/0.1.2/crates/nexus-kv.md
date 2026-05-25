# nexus-kv

> Kind: lib · IPC plugin id: — · CorePlugin: no · Has settings: no · As of: 2026-05-25

## Overview

`nexus-kv` is the on-disk, SQLite-backed implementation of the `KvStore` trait that `nexus-kernel` defines but deliberately does not implement. The kernel owns the *trait* (`crates/nexus-kernel/src/kv_store.rs`) plus a zero-dependency `InMemoryKvStore` fake for tests, but it carries no SQLite dependency itself. `nexus-kv` supplies the real durable backend, `SqliteKvStore`, which `nexus-bootstrap` instantiates and hands to `Kernel::new(config, kv_store)`. This keeps the microkernel backend-agnostic: the kernel holds an `Arc<dyn KvStore>` and never knows whether it is talking to SQLite, a HashMap, or anything else.

The store provides plugin state persistence. Every plugin gets an isolated namespace, and the namespace key is the plugin id — the kernel's `KvAccess` impl on `KernelPluginContext` (`context_impl.rs:428`) calls `self.kv.get(&self.plugin_id, key)` / `set` / `delete`, so a plugin can never read or write outside its own namespace through the public surface. The hot-reload system also uses the store to preserve plugin state across reloads.

In file-as-truth terms, `kv.sqlite3` is **derived/per-machine persistent state**, not source of record. It lives at `<forge>/.forge/kv.sqlite3` and is excluded from git by the default forge `.gitignore` (`nexus-storage/src/forge.rs:189` lists `kv.sqlite3` and `kv.sqlite3-*` under "Per-machine SQLite state"). Unlike the search/index databases it is not rebuildable from the markdown files — it holds ephemeral per-plugin state that simply does not sync across peers. The crate description (Cargo.toml) summarises it as "Nexus KV store backends (SQLite on disk, in-memory for tests) implementing nexus-kernel::KvStore".

## Position in the dependency graph

- **Direct nexus-* dependencies:** `nexus-kernel` only — for the `KvStore` trait and the `KvError` type. (Both are re-exported from the kernel root; `KvError` is defined in `nexus-kernel/src/error.rs:150`.)
- **Notable external dependencies (+ why):**
  - `rusqlite` (workspace pin) — the entire backend. Uses `Connection`, `OptionalExtension` (for `.optional()` on `query_row`), `prepare_cached`, and `params!`. No bundled-SQLite or other features are enabled in this crate's own `Cargo.toml` beyond the workspace defaults.
- **Crates that depend on this one:** Only `nexus-bootstrap` (`crates/nexus-bootstrap/Cargo.toml`). Bootstrap is the sole construction site: `nexus-bootstrap/src/lib.rs:222-226` builds `SqliteKvStore::open(<forge>/.forge/kv.sqlite3)` and wraps it in `Arc<dyn nexus_kernel::KvStore>`. No service crate links `nexus-kv` directly; everyone else reaches KV through the kernel's `Arc<dyn KvStore>` (e.g. `dream_cycle.rs:117` passes `runtime.kernel.kv_store()` around).

## Public API surface

The whole crate is a single module, `crates/nexus-kv/src/lib.rs`. It declares `#![deny(missing_docs)]` and `#![warn(clippy::pedantic)]`. There is exactly one public type.

**`pub struct SqliteKvStore`** — SQLite-backed KV store, thread-safe via an internal `Mutex<Connection>`. It implements a custom `Debug` (`finish_non_exhaustive()`, so the connection is not printed).

Constructors:

- **`pub fn open(path: &Path) -> Result<Self, KvError>`** — opens (or creates) a database file at `path`, applies WAL + `synchronous = NORMAL` pragmas, and runs the `CREATE TABLE IF NOT EXISTS kv_store (...)` migration. Returns `KvError::BackendError { reason }` if the file cannot be opened or the migration fails. This is the production entry point used by bootstrap.
- **`pub fn in_memory() -> Result<Self, KvError>`** — opens a `:memory:` SQLite database and runs the same `CREATE TABLE` migration (but **no** WAL/synchronous pragmas — they are meaningless for an in-memory DB). Documented as being for testing the SQLite code path specifically; the doc comment steers callers toward the kernel's `InMemoryKvStore` when they just need a fast fake rather than the real SQLite engine.

`KvStore` trait methods implemented (all take `&self`, all map backend failures to `KvError::BackendError`):

- **`fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, KvError>`** — `SELECT value FROM kv_store WHERE namespace = ?1 AND key = ?2` via `prepare_cached`, returning `Ok(None)` when the row is absent (uses `.optional()`).
- **`fn set(&self, namespace: &str, key: &str, value: &[u8]) -> Result<(), KvError>`** — upsert: `INSERT ... ON CONFLICT(namespace, key) DO UPDATE SET value = excluded.value`.
- **`fn delete(&self, namespace: &str, key: &str) -> Result<(), KvError>`** — `DELETE FROM kv_store WHERE namespace = ?1 AND key = ?2`. Idempotent: deleting a missing key returns `Ok(())` (no rows-affected check), matching the trait contract.

Note the trait signatures take an explicit `namespace`; the *plugin-id-as-namespace* binding happens one layer up, in the kernel's `KvAccess` impl, not in this crate. `SqliteKvStore` itself is namespace-agnostic and would faithfully store whatever namespace string it is handed.

## IPC handlers

None. `nexus-kv` is a pure backend library — it has no `CorePlugin` impl, registers no IPC commands, defines no plugin id, and is never registered with the kernel's IPC dispatcher. It is reachable only as the concrete type behind the kernel's `Arc<dyn KvStore>`. The IPC/capability-checked surface that *uses* it lives in `nexus-kernel` (`KvAccess::kv_get/kv_set/kv_delete` on `KernelPluginContext`), not here.

## Capabilities

`nexus-kv` performs **no** capability checks. The `get`/`set`/`delete` methods run their SQL unconditionally — any caller holding a reference to the store can read or write any namespace.

Capability gating is enforced **in the kernel**, before a call ever reaches this crate:

- `crates/nexus-kernel/src/context_impl.rs:431` — `kv_get` calls `self.require_capability(Capability::KvRead)?` (`"kv.read"`).
- `context_impl.rs:438` and `:445` — `kv_set` and `kv_delete` call `require_capability(Capability::KvWrite)?` (`"kv.write"`).

The capability string mapping is in `nexus-plugin-api/src/capability.rs:222-223` (`KvRead => "kv.read"`, `KvWrite => "kv.write"`). Both are classified low-risk in `nexus-security/src/risk.rs`. The architectural division is deliberate: the kernel mediates *who* may touch KV and *which namespace* (the plugin id), while `nexus-kv` is the dumb, trusted storage primitive sitting below the capability boundary.

## Settings / Config

No config struct, no TOML file, no `serde(default)` fields — nothing in `docs/0.1.2/settings/`. All behaviour is hardcoded:

- **DB path:** chosen by the caller. In practice `nexus-bootstrap/src/lib.rs:222` hardcodes `<forge>/.forge/kv.sqlite3`. Bootstrap (not this crate) creates the `.forge/` directory first.
- **Pragmas (open() only):** `journal_mode = WAL` and `synchronous = NORMAL`, applied once at open via `execute_batch`. These are not configurable.
- **Connection pooling:** none. A single `rusqlite::Connection` is held behind a `Mutex`; every operation locks it. There is no pool and no per-thread connection. This serialises all KV access through one mutex.
- **Migrations:** a single idempotent `CREATE TABLE IF NOT EXISTS` — there is no versioned migration framework.

## Events

None. The crate neither publishes nor subscribes to any kernel events; it has no access to the event bus.

## Internals & notable implementation details

- **Schema:** one table, `kv_store(namespace TEXT NOT NULL, key TEXT NOT NULL, value BLOB NOT NULL, PRIMARY KEY (namespace, key))`. The composite primary key is what makes namespacing safe and gives the upsert its conflict target.
- **Value serialization:** none. Values are opaque `&[u8]` / `Vec<u8>` stored as a SQLite `BLOB`. Any structuring (JSON, bincode, etc.) is the caller's responsibility. A test confirms full 0..=255 byte roundtrips.
- **Namespacing / scoping:** by the `namespace` column, an exact string match (`namespace = ?1`). Namespaces never collide on shared prefixes because the query uses `=`, not `LIKE` — explicitly regression-tested (see Tests). The plugin id is the namespace in production.
- **Transactions:** each method is a single SQL statement run under the connection mutex; there is no multi-statement transaction or batch API. Atomicity is per-call.
- **Connection management:** one `Connection` for the lifetime of the store, guarded by `Mutex`. Lock-poisoning is converted to `KvError::BackendError { reason: "lock poisoned: ..." }` rather than panicking. `get` uses `prepare_cached` (cached prepared statement); `set`/`delete` use `Connection::execute` directly.
- **Error handling:** every rusqlite failure is funnelled into `KvError::BackendError` with a contextual `reason` string (open, migration, prepare, query, upsert, delete, lock). `KvError::NotFound` (the other `KvError` variant) is never produced by this backend — a missing key surfaces as `Ok(None)` from `get`.
- **WAL artifacts:** WAL mode produces `kv.sqlite3-wal` / `kv.sqlite3-shm` sidecars, which is why the forge `.gitignore` excludes `kv.sqlite3-*` as well as `kv.sqlite3`.

## Tests

All tests are inline in `src/lib.rs` under `mod sqlite_tests`, exercising the SQLite backend via `SqliteKvStore::in_memory()`:

- `get_nonexistent_returns_none` — missing key yields `Ok(None)`.
- `set_and_get_roundtrip` — basic write-then-read.
- `set_overwrites_existing` — upsert semantics (second `set` wins).
- `namespaces_are_isolated` — same key in two namespaces stays distinct.
- `delete_does_not_cross_namespaces` — (Issue #85) deleting in `plugin.a` leaves `plugin.b` untouched; covers the delete path the audit flagged as untested.
- `substring_namespaces_do_not_collide` — `plugin` vs `plugin.foo` do not interfere on get or delete; guards against a naive `LIKE 'plugin%'` regression.
- `empty_namespace_is_distinct_from_others` — `""` is a valid, distinct namespace.
- `delete_removes_key` — delete then get yields `None`.
- `delete_nonexistent_is_ok` — deleting a missing key is `Ok(())` (idempotent).
- `binary_data_roundtrips` — full 0..=255 byte BLOB roundtrip.

There is no separate `tests/` directory — the crate has only `Cargo.toml` and `src/lib.rs`. The parallel `InMemoryKvStore` (the other backend referenced by this crate's docs) is tested separately in `nexus-kernel/src/kv_store.rs::in_memory_tests`, and the kernel's capability-gated `KvAccess` wrapper is tested in `nexus-kernel/src/context_impl.rs` (e.g. `kv_get_set_delete_roundtrip`, plus a denial test asserting a context without `kv.write` errors).
