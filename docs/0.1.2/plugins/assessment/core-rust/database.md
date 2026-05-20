# com.nexus.database

- **Path:** `crates/nexus-database/`
- **Tier:** Core Rust
- **Bootstrap order:** 3 (right after storage)

## Architecture

- Entry point: `crates/nexus-database/src/core_plugin.rs` (`DatabaseCorePlugin`; `#[derive(Default)]`, stateless).
- Bootstrap wiring: `crates/nexus-bootstrap/src/plugins/database.rs:18` — `LifecycleFlags::NONE`, manifest from `IPC_HANDLERS`, no `plugin.toml` file.
- Pure-logic crate: types, validation, formulas, CSV import/export, view-application engine, relation resolution. **No SQL** — the SQL-backed bases query/index lives in `nexus-storage::bases` (per `crates/nexus-bootstrap/src/plugins/database.rs:6` and `Cargo.toml` description).
- Key modules:
  - `formula/` — Notion-compatible formula language and evaluator (`formula::evaluate`).
  - `import_export.rs` — `import_csv` / `export_csv` with `ColumnMapping`.
  - `views.rs` — `apply_view` (filter / sort / group pipeline returning `AppliedView`).
  - `relations.rs` — `resolve_relation`, `compute_rollup`, `parse_aggregation` (PRD-10 §7).
  - `validate.rs`, `types.rs`.
- Persistence: none. Every handler is a pure function over its args.
- Settings owned: none.
- External dependencies of note: `csv`, `chrono`, `uuid`, `regex-lite`. No `rusqlite`, no kernel, no event bus.

## Surface

IPC commands (from `core_plugin.rs:55` `IPC_HANDLERS`):

| Id | Command            | Purpose                                                       |
|---:|--------------------|---------------------------------------------------------------|
|  1 | `csv_import`       | Parse CSV bytes into `BaseRecord`s (10 MiB input cap)         |
|  2 | `csv_export`       | Serialize `BaseRecord`s to CSV bytes                          |
|  3 | `formula_eval`     | Evaluate a formula against a record's fields                  |
|  4 | `apply_view`       | Run filter / sort / group pipeline, return `AppliedView`      |
|  5 | `resolve_relation` | Match related records by relation definition                  |
|  6 | `compute_rollup`   | Aggregate a field across related records                      |

No events. No lifecycle hooks. The plugin holds no state.

## Necessity

- **Verdict:** Optional (basic capabilities) / Essential (as a library).
- **Required for basic capabilities?** No — opening a forge, browsing markdown, editing, search, and git work without ever calling these handlers. The handlers serve the bases / database-view feature surface (PRD-10) and the editor's inline `[[{db:query}]]` blocks (BL-012). Without bases content in the forge, the plugin is idle.
- **Depended on by:** `nexus-storage` Cargo-depends on this crate for `BaseRecord` validation and the views/relations/formula helpers. `nexus-editor::execute_database_view` (handler 12) routes through `com.nexus.database::apply_view` over IPC (`crates/nexus-editor/src/core_plugin.rs:35`).
- **Depends on:** `nexus-types`, `nexus-plugins`. No kernel dep.
- **What breaks if removed (plugin):** inline database views, CSV import/export, formula evaluation, and the rollup/relation handlers stop working — the editor's `execute_database_view` and storage's `base_query`/`apply_view` rely on this surface. The basic markdown workflow is unaffected.

## Notes

- The crate description (`Cargo.toml:7`) flags the boundary explicitly: "No SQL — SQL-backed query/schema lives in `nexus-storage::bases`."
- `csv_import` enforces `MAX_CSV_IMPORT_BYTES = 10 MiB` to bound DoS exposure from full in-memory parsing (issue #78).
- Handler ids are append-only (`core_plugin.rs:33` comment).
- `ts-export` feature emits JSON Schema + TS bindings for arg/response DTOs (P1-3 / issue #113), excluding response types that wrap `BaseRecord` due to its `#[serde(flatten)]`.
