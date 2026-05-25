# nexus-database

> Kind: lib · IPC plugin id: com.nexus.database · CorePlugin: yes · Has settings: no · As of: 2026-05-25

## Overview

`nexus-database` is the pure-logic library behind Nexus's database / "bases" feature — the Notion-style structured-data layer that turns a forge's markdown into typed tables with rich property types, formulas, filtered/sorted/grouped views, cross-base relations, and CSV import/export. Crucially, **it does not touch `rusqlite` or the forge's SQLite database**. The crate's own module doc states this explicitly: the SQL-backed query engine, schema migrations, and the storage-level relation/rollup persistence that *used* to live here were moved into `nexus-storage` (`nexus_storage::bases::{schema, query, relation}`) so that `nexus-storage` remains the sole owner of `.forge/index.db`. Everything in `nexus-database` is a pure function over in-memory data: it parses, validates, evaluates, and transforms `BaseRecord`/`BaseSchema`/`BaseView` values that are handed to it, and hands results back.

The data model has two halves. `PropertyConfig` (in `types.rs`) is the rich, typed configuration of a single field — 20 variants covering text, number/currency/percent, date/time/datetime, single/multi-select, relation, rollup, formula, lookup, uuid, url, email, phone — and is serialized as JSON inside a base's schema. `PropertyValue` is the engine's typed view of an actual value, freely convertible to/from `serde_json::Value` for storage in `records.json` (file-as-truth). On top of these sit four pure subsystems: type-aware validation (`validate.rs`), a Notion-compatible formula language (`formula/`), a filter/sort/group view pipeline (`views.rs`), and cross-base relation resolution + rollup aggregation (`relations.rs`), plus CSV import/export (`import_export.rs`).

The crate exposes a thin `DatabaseCorePlugin` (id `com.nexus.database`) that surfaces six of these pure helpers over IPC. This is the microkernel discipline at work (invariant #3): frontends — CLI, TUI, MCP, shell — never link `nexus-database` directly; they call `ipc_call("com.nexus.database", command, args)`. The plugin holds no state — every handler is a pure function over its decoded args. Callers that need an actual SQL scan against persisted bases go to `ipc_call("com.nexus.storage", "base_query", …)` instead; the division of labour is "shape and compute here, persist and query there." A typical UI flow: storage loads the records and schema, `com.nexus.database::apply_view` filters/sorts/groups them in memory, and the shell renders the resulting `AppliedView`.

The crate has an optional `ts-export` feature that emits TypeScript bindings (via ts-rs) and JSON Schema (via schemars) for the IPC arg/reply DTOs that *don't* reference `nexus_types::bases::BaseRecord` — `BaseRecord` is excluded because its `#[serde(flatten)]` forward-compat fields are incompatible with the `deny_unknown_fields` gate the export pipeline enforces.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-types` (provides `bases::{BaseRecord, BaseSchema, BaseView, BaseRelation, FilterRule, SortRule, ViewType, BasesError}`), `nexus-plugins` (the `CorePlugin` trait, `PluginError`, and the `define_dispatch_helpers!` macro). Both are leaf-ish crates — the kernel-isolation invariant holds (this crate never depends on the kernel or on `nexus-storage`).
- **Notable external deps:** `regex-lite` (email validation), `csv` (import/export reader/writer), `chrono` (date/time parsing + formula date functions), `uuid` (record-id generation on import), `serde`/`serde_json`, `thiserror`, `tracing`. Optional behind `ts-export`: `ts-rs`, `schemars`.
- **Dev deps:** `tempfile`.
- **Crates depending on it:** `nexus-bootstrap` registers `DatabaseCorePlugin` (`crates/nexus-bootstrap/src/plugins/database.rs`). No subsystem crate links it for direct calls — by design, access is IPC-only.

## Public API surface

### `types.rs` — property type system
- `PropertyConfig` — tagged (`#[serde(tag = "type", rename_all = "kebab-case")]`) enum, 20 variants: `Text`/`LongText` (optional `max_length`), `Number` (`format`/`min`/`max`), `Currency` (`symbol`/`decimal_places`), `Percent`, `Checkbox`, `Date`/`Datetime` (`format`), `Time`, `Select`/`MultiSelect` (`options`), `Relation` (`target_database`), `Rollup` (`relation_property`/`target_property`/`aggregation`), `Formula` (`expression`), `Lookup`, `Uuid`, `Url`, `Email`, `Phone`.
- `SelectOption` — `{ id, name, color? }`; stable `id` survives renames.
- `NumberFormat` (enum: Number, NumberWithCommas, Percent, Dollar, Euro, Pound, Yen; default Number).
- `DateFormat` (enum: Full, Relative, YearMonthDay, MonthDayYear, DayMonthYear; default Full).
- `RollupAggregation` (enum, snake_case serde: Sum, Average, Min, Max, Count, CountUnique, CountValues, CountEmpty, CountNotEmpty, PercentEmpty, PercentNotEmpty).
- `PropertyValue` — untagged enum (Null, Boolean, Number, Text, TextArray) with `from_json`, `to_json`, `as_display_string`, `as_number` (coerces text/bool), `is_empty`.

### `error.rs`
- `DatabaseError` — `ValidationFailed{field,reason}`, `SchemaError`, `QueryError`, `FormulaError{position,message}`, `RelationError`, `ImportExportError`, `Bases(#[from] BasesError)`, `Io(#[from])`. Plus `type Result<T>`.

### `validate.rs` — type-aware validation
- `Severity` (Error/Warning), `ValidationIssue{field,severity,message}`.
- `PropertyValidator` trait (`Send + Sync`) — `validate(field, value, config)`.
- `BuiltinValidator` — the default impl covering all property types (null always passes; computed/relation/uuid types skip).
- `validate_record_full(record, configs, validator) -> Vec<ValidationIssue>` — runs type-aware validation on every present field that has a config; skips `id`; collects all issues (no short-circuit).

### `formula/` — Notion-compatible formula language
- `formula::evaluate(expression, fields) -> Result<FormulaValue>` — top-level entry (re-exported at crate root as `evaluate_formula`): tokenize → parse → evaluate.
- `token` — `Token`, `Spanned`, `tokenize(input) -> Result<Vec<Spanned>>`.
- `ast` — `Expr` (Literal, PropertyRef, FunctionCall, BinaryOp, UnaryOp, If), `LiteralValue`, `BinaryOp`, `UnaryOp`.
- `parser` — `parse(tokens) -> Result<Expr>`; recursive-descent with documented precedence.
- `eval` — `FormulaValue` (Null, Number, String, Boolean, Date, Array) with `as_number`, `is_truthy`, `to_display_string`, custom `PartialEq`/`Display`; `EvalContext{fields}`; `evaluate(expr, ctx)`; `MAX_RECURSION_DEPTH = 64`.
- `functions::call(name, args) -> Result<FormulaValue>` — the built-in library (see Internals).

### `views.rs` — filter / sort / group pipeline
- `apply_view(records, schema, view) -> AppliedView` — pure; `schema` accepted but currently unused (reserved for future type-aware filters).
- `AppliedView{view_name, view_type, fields, layout}`, `ViewLayout` (`Flat{records}` | `Grouped{groups}`), `ViewGroup{key, records}`.
- `validate_filter_operator(op) -> bool` — load-time operator validity check.
- `MISSING_GROUP_KEY = "(none)"`.

### `relations.rs` — cross-base relations + rollups (PRD-10 §7 / DG-41)
- `resolve_relation(source, relation, target_records) -> Result<Vec<&BaseRecord>, RelationError>` — pure; filters soft-deleted targets, dedupes by id, preserves order.
- `compute_rollup(source, relation, aggregate_field, aggregation, target_records) -> Result<serde_json::Value, RelationError>`.
- `parse_aggregation(s) -> Option<RollupAggregation>` — case-insensitive, accepts snake_case + a few aliases (`avg`/`mean`).
- `RelationError` (MissingSourceField, UnsupportedSourceShape).

### `import_export.rs` — CSV
- `import_csv(reader, mapping, has_header) -> Result<(Vec<BaseRecord>, ImportResult)>` — generates a UUID per row, auto-detects bool/number/string cell types, returns counts + per-row errors. Caller persists.
- `export_csv(writer, records, field_names) -> Result<usize>` — header + one row per record.
- `ColumnMapping{mappings}` with `from_headers(headers, field_names)`; `ImportResult{imported, skipped, errors}`.

### `core_plugin.rs`
- `DatabaseCorePlugin` (impl `CorePlugin`), `PLUGIN_ID = "com.nexus.database"`, handler-id consts, `IPC_HANDLERS: &[(&str, u32)]` (SD-06 single source of truth), and the arg/response DTOs (`CsvImportArgs`/`Response`, `CsvExportArgs`/`Response`, `FormulaEvalArgs`/`Response`, `ApplyViewArgs`, `ResolveRelationArgs`, `ComputeRollupArgs`).

## IPC handlers

Registered by `nexus-bootstrap` from `IPC_HANDLERS`; each command also gets a `<name>.v1` alias (`with_v1_aliases`). Handler ids are append-only and never reused.

| command | id | args | returns | capability | description |
|---------|----|------|---------|------------|-------------|
| `csv_import` | 1 | `CsvImportArgs { csv_bytes: Vec<u8>, field_names: Vec<String>, has_header: bool, column_mapping?: Vec<(usize,String)> }` | `CsvImportResponse { records: Vec<BaseRecord>, imported, skipped, errors: Vec<(usize,String)> }` | none (unrestricted) | Parse CSV bytes into `BaseRecord`s. Derives a column mapping from headers (or positional) when none supplied. Rejects input over 10 MiB (`MAX_CSV_IMPORT_BYTES`, issue #78). |
| `csv_export` | 2 | `CsvExportArgs { records: Vec<BaseRecord>, field_names: Vec<String> }` | `CsvExportResponse { csv_bytes: Vec<u8>, count }` | none (unrestricted) | Serialize records to CSV bytes (header + one row per record). |
| `formula_eval` | 3 | `FormulaEvalArgs { expression: String, fields: Map<String,Value> }` | `FormulaEvalResponse { display: String }` | none (unrestricted) | Tokenize/parse/evaluate a formula against a record's fields; returns the display-formatted result. |
| `apply_view` | 4 | `ApplyViewArgs { records: Vec<BaseRecord>, schema: BaseSchema, view: BaseView }` | `AppliedView` (JSON: `{ view_name, view_type, fields, layout }`) | none (unrestricted) | Run the full filter → sort → group pipeline; returns a flat or grouped layout. |
| `resolve_relation` | 5 | `ResolveRelationArgs { source: BaseRecord, relation: BaseRelation, target_records: Vec<BaseRecord> }` | `Vec<BaseRecord>` (matched targets) | none (unrestricted) | Resolve a relation field to its target records (dedup, soft-delete filtered). |
| `compute_rollup` | 6 | `ComputeRollupArgs { source, relation, target_records, aggregate_field: String, aggregation: String }` | `serde_json::Value` (number/null per aggregation) | none (unrestricted) | Resolve the relation, then aggregate `aggregate_field` over the related set. `aggregation` parsed via `parse_aggregation` (case-insensitive). |

Unknown handler ids return `PluginError::ExecutionFailed`. (`docs/0.1.2/ipc-handlers.md` lists this plugin as 6 handlers, all unrestricted — matches source.)

## Capabilities

None. No handler performs a capability check; `docs/0.1.2/ipc-handlers.md` classifies all six as `unrestricted` ("pure compute and serialization"). Any persistence side-effect (`fs.write`) happens downstream in `nexus-storage`, which enforces its own caps. The plugin registers with `LifecycleFlags::NONE`.

## Settings / Config

None. There is no `Config` struct, no `serde(default)` settings field, and no `.forge/` TOML owned by this crate. All defaults that exist (`default_currency_symbol = "$"`, `default_decimal_places = 2`, `MAX_RECURSION_DEPTH = 64`, `MAX_CSV_IMPORT_BYTES = 10 MiB`) are hardcoded constants / serde defaults inside the property types and handlers, not user-configurable knobs.

## Events

None. The crate neither publishes nor subscribes to the event bus. It imports no `EventBus`; the only event-bus interaction is in bootstrap registration (`or_lifecycle_skip`), which is generic plumbing, not specific to this crate.

## Internals & notable implementation details

**Property type system.** `PropertyConfig` round-trips through JSON with a `type` tag. `PropertyValue::from_json` is lossy-but-forward-compatible: JSON objects are stringified into `Text` rather than rejected. `as_number` coerces text and booleans (true→1.0). The `_config` argument to `from_json` is currently unused — conversion is driven purely by the JSON value's shape, not the declared type.

**Validation.** `BuiltinValidator` dispatches on `PropertyConfig`. Null always passes (required-ness is a separate, storage-level concern). Email uses a `regex-lite` pattern; URL requires an `http(s)://` prefix; phone requires 7–15 ASCII digits after stripping non-digits; select/multi-select accept a value matching either an option's `id` *or* `name`. Computed/derived types (`Formula`, `Rollup`, `Lookup`, `Uuid`, `Relation`) skip validation entirely. `validate_record_full` collects *all* issues (no short-circuit) and skips the system `id` field.

**Formula parser/evaluator.** Hand-written tokenizer + recursive-descent parser. Precedence (low→high): `or`, `and`, `==`/`!=`, comparisons, `+`/`-`, `*`/`/`/`%`, unary `-`/`not`, then primaries. Two special forms are desugared at parse time: `prop("field")` → `PropertyRef`, and `if(c, t, e)` → `If`. A bare identifier is shorthand for `prop("identifier")`. A lone `=` is accepted as `==`. The evaluator caps recursion at `MAX_RECURSION_DEPTH = 64` (issue #78 — pathological nested ASTs would otherwise blow the stack). `+` is overloaded: string concat if either operand is a string, else numeric add. Comparison operators coerce both sides numerically when possible (via a zero-padded fixed-width string `{:020.10}` so f64s sort correctly), else compare lexicographically. Division/modulo by zero is an error. Truthiness: null/empty-string/zero/empty-array are falsy.

Built-in functions (`functions::call`, dispatched by lowercase name): **string** — `concat` (varargs), `upper`, `lower`, `trim`, `len`/`length`, `replace`, `slice` (2–3 args), `contains`, `starts_with`, `ends_with`; **numeric** — `abs`, `round` (1–2 args), `floor`, `ceil`, `sqrt`, `pow`, `min`, `max`; **date** (chrono-backed, ISO `%Y-%m-%dT%H:%M:%S`) — `now`, `year`, `month`, `day`, `dateAdd`/`date_add` (days/weeks/hours), `dateBetween`/`date_between` (days/hours/minutes), `toDate`/`to_date`; **conversion** — `toNumber`/`to_number`, `toString`/`to_string`, `empty`; **logical** — `and`, `or`, `not` (also available as operators). Unknown functions and arg-count/type mismatches return descriptive `FormulaError`s.

**Views.** `apply_view` is a three-stage pure pipeline: filter (all rules must pass — AND semantics) → stable multi-key sort → optional grouping. Filter operators: `eq`/`=`, `neq`/`!=`, `gt`/`gte`/`lt`/`lte` (+ symbol forms), `contains`/`icontains`, `starts_with`/`ends_with`, `is_empty`/`is_not_empty`, `in`. Unknown operators silently return `false` (drop the record) — hence `validate_filter_operator` for load-time checks. Sorting always sinks nulls to the bottom regardless of asc/desc; non-null comparison tries numeric, then bool, then string. Grouping: Kanban/List/Timeline group on `group_field` (falling back to `Flat` if absent); Calendar groups on `date_field` bucketed by `YYYY-MM-DD` prefix; Table/Gallery are always flat. Group keys are produced via `BTreeMap`, so groups come out in sorted key order; missing/empty keys collapse into the `"(none)"` sentinel. Multi-select array keys are joined with `, `.

**Relations & rollups.** `resolve_relation` reads the source record's `source_field` (accepts scalar string/number or an array of them — mirroring many-to-one vs many-to-many), then matches target records on `target_field` (special-cased to the typed `BaseRecord.id` when `target_field == "id"`). Soft-deleted targets (`deleted_at.is_some()`) are filtered out; results are deduped by id preserving order. `compute_rollup` resolves first, then aggregates the projected `aggregate_field`: count-family variants operate on set size / null counts / distinct counts; sum/avg/min/max coerce via `as_f64` and ignore nulls; percent variants compute null-vs-non-null ratios. Empty inputs yield `null` for numeric aggregations. Results ride over IPC as raw JSON to avoid re-coupling to the formula engine.

**CSV.** Import generates a `uuid::Uuid::new_v4()` per row (also stored under an `id` field in `fields`), auto-detects cell types (`true`/`false` → bool, parseable → f64, empty → null, else string), and reports per-row parse errors with 1-based row numbers (+1 for the header). Export writes a header then projects each record's fields in `field_names` order; arrays are joined with `; `, objects stringified. The IPC `csv_import` handler enforces the 10 MiB cap *before* parsing and builds the `ColumnMapping` from explicit pairs, header match, or positional indices.

**Query handoff to storage.** This crate never runs SQL. The module docs repeatedly point callers at `com.nexus.storage` (`base_index`/`base_list`/`base_query`) for index-accelerated scans and persistence; `nexus-database` only operates on already-loaded in-memory record slices.

## Tests

Unit tests are co-located with each module (all `#[cfg(test)] mod tests`):
- `types.rs` — config serde round-trips, `PropertyValue` conversions/coercions, enum defaults.
- `validate.rs` — per-type validation (email/url/phone/number-bounds/date/select/multi-select/text-length/checkbox), null-passes, formula-skips, and `validate_record_full` collecting multiple issues.
- `formula/token.rs` — tokenizing arithmetic, strings + escapes, comparisons, function calls, boolean keywords, unterminated-string error, single-`=`, decimals.
- `formula/parser.rs` — literals, prop refs (explicit + bare), precedence, parentheses, function calls (incl. nested), `if`, unary ops, error on unexpected token.
- `formula/eval.rs` — arithmetic, string concat, comparisons, logical ops, `if`, property lookup, missing-property-is-null, division-by-zero, null falsy.
- `formula/functions.rs` — every built-in family plus error cases (unknown fn, wrong arg count, type mismatch).
- `import_export.rs` — basic import, type auto-detection, export, full round-trip, header mapping.
- `relations.rs` — scalar/array resolution, order preservation, soft-delete filtering, dedup, null/missing source handling, non-id target field, and every rollup aggregation + `parse_aggregation` case-insensitivity.
- `views.rs` — table flat order, each filter operator, asc/desc + nulls-last sort, multi-level sort, Kanban grouping (incl. `(none)` bucket and flat fallback), Calendar date bucketing, Gallery flat, operator validation, unknown-operator-drops-all, and the combined filter→sort→group pipeline.
- `core_plugin.rs` — dispatch round-trips for csv_import/csv_export/formula_eval, the kanban `apply_view` grouped response, and unknown-handler-id error.

Integration test: `tests/issue_78_bounds.rs` — regression coverage for issue #78 (DoS-shaped unbounded parsing): the evaluator rejects a hand-built 256-deep `If` AST (past the 64 cap), accepts bounded nesting, and the `csv_import` handler rejects an 11 MiB payload via the size gate. (Note: this test depends on `tests/issue_78_bounds.rs` accessing `nexus_database::formula::ast`/`eval` and `core_plugin::HANDLER_CSV_IMPORT` as public API.) Bootstrap-level IPC wiring is additionally exercised in `nexus-bootstrap/tests/database_ipc.rs`.
