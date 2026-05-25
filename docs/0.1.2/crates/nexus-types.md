# nexus-types

> Kind: lib · IPC plugin id: — · CorePlugin: no · Has settings: no · As of: 2026-05-25

## Overview

`nexus-types` is the **leaf of the Nexus dependency graph**. It holds types and pure-logic helpers that must be shared between the kernel (`nexus-kernel`) and plugin code that runs in WASM sandboxes, and between subsystem crates that would otherwise form a dependency cycle. It declares no `nexus-*` dependencies of its own, which is precisely what lets it sit underneath every other crate without inverting the microkernel layering. Its `lib.rs` carries `#![deny(missing_docs)]` and `#![warn(clippy::pedantic)]`.

Its responsibilities are deliberately narrow: (1) the **universal activity-timeline types** (BL-052) — the single audit surface every emitter publishes to; (2) **forge-relative path confinement** — two distinct path validators (`ForgePathValidator` and `paths::resolve_within`) that gate file operations so callers can't escape the forge root; (3) the **`.bases` directory format** and the parallel single-file **Obsidian `.base` (YAML) format** plus a pure filter-expression evaluator; (4) **canonical plugin identifiers** (`com.nexus.*` reverse-DNS constants); and (5) **shared constants** — IPC timeouts and kernel pool sizing — so frontends don't drift on what "long" means for a round-trip.

It upholds the architecture in several concrete ways. **Microkernel isolation:** because `nexus-kernel` may depend only on `nexus-types` and `nexus-plugin-api` (both leaves), the path validators and constants the kernel needs are placed here rather than in a higher-level crate — `path_validator.rs` documents this explicitly, noting that `nexus-security` re-exports `ForgePathValidator` and layers policy-aware error conversions on top. **Capabilities-gate / file-as-truth:** the two path validators are the mechanical enforcement point behind the kernel's path-confinement (an attacker-supplied relpath can't escape the forge), and the `.bases` / `.base` helpers operate directly on on-disk files (treated as the source of truth), keeping the SQLite/Tantivy index as rebuildable derived state.

The crate registers **no `CorePlugin`** and **no IPC handlers** — it is a pure types-and-logic library consumed by ~18 other crates (every kernel/service crate plus `nexus-fuzz`).

## Position in the dependency graph

- **Direct nexus-* dependencies:** none. This is the leaf crate by design; adding a `nexus-*` dependency here would be an architecture violation.
- **Notable external dependencies (+ why):**
  - `serde` / `serde_json` — derive + the `serde_json::Value`/`Map` used as the open-ended field/property bag in `bases` and `obsidian_base`.
  - `serde_yml` — parse/serialize the Obsidian single-file `.base` YAML format.
  - `toml` — `views.toml` / `relations.toml` inside a `.bases` directory, plus the TOML↔JSON conversion helpers.
  - `thiserror` — `BasesError`, `PathError`, `PathValidationError`, `ObsidianBaseError`.
  - `chrono` — RFC3339 timestamps in `ActivityEntry::now`.
  - `uuid` — v4 ids for `ActivityEntry`.
  - `ts-rs` + `schemars` — **optional**, gated behind the `ts-export` feature; emit TypeScript bindings + JSON Schema for the activity types only.
  - `tempfile` — dev-dependency, used by the `bases` and `path_validator` tests.
- **Crates that depend on this one:** broadly. `nexus-kernel`, `nexus-plugins`, `nexus-security`, `nexus-storage`, `nexus-ai`, `nexus-ai-runtime`, `nexus-git`, `nexus-editor`, `nexus-terminal`, `nexus-workflow`, `nexus-database`, `nexus-notifications`, `nexus-crdt`, `nexus-mcp`, `nexus-cli`, `nexus-tui`, `nexus-fuzz`, and `nexus-bootstrap` all list it as a direct dependency. It is a foundational crate; treat changes to its public types as IPC-boundary changes (the activity types in particular flow through `ipc_call` and are ts-rs-exported).

## Public API surface

### `lib.rs`
Re-exports `path_validator::{ForgePathValidator, PathValidationError}` at the crate root for convenience. Declares the seven public modules below.

### `mod activity` — universal activity-timeline types (BL-052)
The single audit surface: every observable side effect (user, agent, AI, or plugin) lands here. Originally introduced in `nexus-ai` for AI calls (BL-037), then lifted to this leaf crate so terminal/git/storage/workflow/capability subsystems can publish without depending on `nexus-ai`.

| Item | Kind | Purpose |
|------|------|---------|
| `ACTIVITY_APPENDED_TOPIC` | `const &str` = `"com.nexus.activity.appended"` | Kernel-owned bus topic; every emitter publishes here. |
| `AI_ACTIVITY_APPENDED_TOPIC` | `const &str` = `"com.nexus.ai.activity_appended"` | Legacy AI-only topic kept alive by `nexus-ai`'s recorder for pre-BL-052 subscribers. |
| `ACTIVITY_PROMPT_MAX_CHARS` | `const usize` = `256` | Hard cap on stored prompt text; truncation happens at emit time. |
| `ActivitySurface` | enum | Surface that originated the entry (`Chat`/`Ask`/`CmdI`/`Ghost`/`Complete`/`Enrich`/`File`/`Process`/`Git`/`Workflow`/`Capability`/`Other`). `lowercase` serde rename. |
| `ActivitySurface::from_str_lossy(&str) -> Self` | fn | Tolerant parse; unknown values → `Other` (also accepts `cmd-i`/`cmd_i` aliases for `CmdI`). |
| `ActivityOutcome` | enum | `Ok` / `Error` / `Cancelled`, `lowercase` serde rename. |
| `ActivityOrigin` | enum | Structured origin: `Ai`, `User`, `Plugin(String)`, `Workflow(String)`, `Agent(String)`, `Terminal(String)`, `Git`, `Storage`, `Capability`. **Not** serde-derived — serialized as a wire string. |
| `ActivityOrigin::to_wire(&self) -> String` | fn | Render to wire form (`ai`, `user`, `plugin:<id>`, `workflow:<run_id>`, …). |
| `ActivityOrigin::from_wire(&str) -> Self` | fn | Tolerant parse; unknown kinds fall back to `Plugin(<full_string>)` (lossless round-trip). |
| `ActivityOrigin::kind(&self) -> &'static str` | fn | Prefix kind (part before `:`) for the shell's origin filter chip. |
| `ActivityToolCall` | struct | One tool call / sub-step: `name: String`, `ok: bool`. `deny_unknown_fields`. |
| `ActivityEntry` | struct | One timeline entry (id, timestamp, session_id, surface, origin, provider, model, prompt, files, tool_calls, outcome, error, duration_ms). `deny_unknown_fields`. |
| `ActivityEntry::now(session_id, surface, origin) -> Self` | fn | Construct a minimal entry (fresh UUID + RFC3339 now). |
| `ActivityEntry::now_ai(session_id, surface) -> Self` | fn | Back-compat constructor; defaults origin to `Ai`. |
| `truncate_prompt(&mut String, max_chars)` | fn | Unicode-safe in-place truncation with an `…` suffix; shared by every emitter so the bound is uniform. |

### `mod bases` — Nexus `.bases` directory format + filesystem helpers
Shared so non-storage consumers (database engine, CLI) can work with `.bases` directories without pulling a SQLite dep.

| Item | Kind | Purpose |
|------|------|---------|
| `Base` | struct | Full loaded base: `name`, `schema`, `records`, `views`, `relations`, `metadata`. |
| `BaseSchema` | struct | `version` (default `"1.0"`) + `fields: serde_json::Map`. |
| `BaseRecord` | struct | `id`, `deleted_at` (serde `deletedAt`, soft-delete epoch secs), `#[serde(flatten)] fields`. |
| `BaseView` | struct | `name`, `view_type` (serde `type`), `fields`, `sort`, `filter`, `group_field`, `date_field`, `end_field`. |
| `ViewType` | enum | `Table`/`Kanban`/`Calendar`/`Gallery`/`List`/`Timeline`, `lowercase` rename. |
| `SortRule` | struct | `field` + `direction` (default `"asc"`). |
| `FilterRule` | struct | `field`, `operator` (intentionally `String`), `value`. |
| `BaseRelation` | struct | `name`, `relation_type`(serde `type`), `source_field`, `target_base`, `target_field`. |
| `BaseMetadata` | struct | `version`/`created_at`/`modified_at`; `#[serde(default)]` with a custom `Default` (`version="1.0"`, timestamps 0). |
| `BaseSummary` | struct | Listing row: `id`, `path`, `name`, `record_count`. |
| `FieldDefinition` | struct | A single field's typed definition (`field_type`, `required`, `primary`, `options`, `min`, `max`, `target`, `target_field`). |
| `FieldType` | enum | 18 variants (`text`/`long-text`/`number`/`currency`/`percent`/`checkbox`/`date`/`time`/`datetime`/`select`/`multi-select`/`relation`/`formula`/`rollup`/`lookup`/`uuid`/`url`/`email`), `kebab-case` rename. |
| `BasesError` | enum | `Io`, `FileNotFound`, `CorruptFile{path,reason}`, `ValidationFailed`. |
| `load_base(&Path) -> Result<Base, BasesError>` | fn | Read `schema.json` (required), `records.json`/`views.toml`/`relations.toml`/`metadata.json` (optional). |
| `save_base(&Path, &Base) -> Result<(), BasesError>` | fn | Write all constituent files; deletes `views.toml`/`relations.toml` when their lists are empty. |
| `init_base(&Path, name, &BaseSchema) -> Result<Base, BasesError>` | fn | Create an empty base dir with current timestamps. |
| `validate_record(&BaseSchema, &BaseRecord) -> Result<(), BasesError>` | fn | **Required-field presence only** — does not check types/min/max/options. |

### `mod constants` — shared magic numbers

| Item | Value | Purpose |
|------|-------|---------|
| `IPC_TIMEOUT_SHORT` | 30s | Interactive CLI/UI round-trips. |
| `IPC_TIMEOUT_NORMAL` | 60s | Service plugins making a single outbound hop (MCP bridging). |
| `IPC_TIMEOUT_LONG` | 120s | Model/network IO or larger filesystem work (AI calls, graph rebuild). |
| `IPC_TIMEOUT_EXTENDED` | 600s | Long-running orchestration (agent runs, full-forge sync, workflow runs). |
| `AUDIT_LOG_RETENTION_DAYS` | 90 | Audit-log pruning horizon. |
| `COMMAND_PALETTE_MAX_RESULTS` | 50 | Command-palette result clamp. |
| `KERNEL_BLOCKING_POOL_SIZE` | 64 | `max_blocking_threads` the frontend passes to the tokio runtime; caps concurrent sync IPC + all blocking work. |
| `KERNEL_BLOCKING_POOL_WARN_DEPTH` | 48 | Warn threshold (75% of pool) for sustained sync-dispatch depth. |
| `SLOW_SYNC_DISPATCH_WARN` | 500ms | Per-call warn threshold for a single slow sync dispatch holding the backend mutex. |

### `mod obsidian_base` — Obsidian single-file `.base` (YAML) format (ADR 0019)
Parallel format to `.bases`. Records are **not** stored on disk; they're computed by querying the vault at view time. This module owns the *file shape* only.

| Item | Kind | Purpose |
|------|------|---------|
| `ObsidianBase` | struct | `filters: Option<FilterNode>`, `properties: serde_json::Map`, `views: Vec<ObsidianView>`. `Default` + `PartialEq`. |
| `ObsidianView` | struct | `name`, `view_type`(serde `type`, kept as `String` so unknown types round-trip), `order`, `sort`, `filters`, `group_by`(serde `groupBy`), `limit`. |
| `SortDirection` | enum | `Asc`(default)/`Desc`, `UPPERCASE` rename. |
| `ObsidianSort` | struct | `property` + `direction`. |
| `FilterNode` | enum (untagged) | Boolean tree: `And{and}` / `Or{or}` / `Not{not: Box}` / `Expr(String)`. Leaf expressions kept opaque so unsupported grammar still round-trips. |
| `ObsidianBaseError` | enum | `Parse(#[from] serde_yml::Error)`. |
| `parse(&str) -> Result<ObsidianBase, ObsidianBaseError>` | fn | Parse YAML. |
| `to_yaml(&ObsidianBase) -> Result<String, ObsidianBaseError>` | fn | Serialize back to YAML. |

### `mod obsidian_base::filter` — filter-expression evaluator
Pure logic; no I/O, no SQLite. Evaluates a `FilterNode` tree against one note's facts. The SQLite-backed query that builds `NoteFacts` from the index lives in `nexus-storage`.

| Item | Kind | Purpose |
|------|------|---------|
| `NoteFacts` | struct | All facts for one note: `name`, `path`, `ext`, `folder`, `ctime`, `mtime`, `tags: Vec<String>`, `frontmatter: BTreeMap<String, Value>`. |
| `EvalReport` | struct | `matched: bool` + `unsupported: Vec<String>` (distinct unsupported expressions encountered). |
| `evaluate(Option<&FilterNode>, &NoteFacts) -> EvalReport` | fn | Walk the tree; `None` node = match everything. |

The grammar v1 (`Expr`, `BinOp`, `Method`, `Lhs`, `FileIntrinsic`, `Literal`, the recursive-descent parser, and the evaluator) is **all private** — only `NoteFacts`, `EvalReport`, and `evaluate` are public.

### `mod path_validator` — forge-root path validation + symlink enforcement

| Item | Kind | Purpose |
|------|------|---------|
| `ForgePathValidator` | struct | Constructed once per forge; canonicalizes the root at creation. Immutable/thread-safe. |
| `ForgePathValidator::new(&Path)` | fn | Canonicalizes `forge_root` (must exist on disk). |
| `ForgePathValidator::forge_root(&self) -> &Path` | fn | The canonical root. |
| `ForgePathValidator::validate(&self, &Path)` | fn | **Permissive read** validator: strips leading `/`, drops `.`, allows in-root `..`, canonicalizes (follows symlinks), verifies inside root. Target must exist. |
| `ForgePathValidator::validate_for_write(&self, &Path)` | fn | **Write** validator: target need not exist; canonicalizes the deepest existing ancestor and rebuilds the non-existing tail onto it, closing a TOCTOU race on the parent. |
| `PathValidationError` | enum | `PathTraversal(PathBuf)`, `InvalidPath(String)`. Dependency-free so low-level crates can use it. |

### `mod paths` — strict component-only path confinement

| Item | Kind | Purpose |
|------|------|---------|
| `resolve_within(&Path, &str) -> Result<PathBuf, PathError>` | fn | **Strict** validator: accepts only `Component::Normal`; rejects leading `/`, `.`, `..`, root dirs, and Windows prefixes. No I/O, no symlink resolution. Empty relpath → root. |
| `PathError` | enum | `Invalid(String)`. |

### `mod plugin_ids` — canonical reverse-DNS plugin identifiers
27 `pub const &str` constants of the form `com.nexus.<name>`: `KERNEL`, `STORAGE`, `AI`, `AI_RUNTIME` (`com.nexus.ai.runtime`), `AGENT`, `COMMENTS`, `EDITOR`, `GIT`, `LINKPREVIEW`, `MCP` (`com.nexus.mcp.host`), `LSP`, `DAP`, `ACP`, `SKILLS`, `TEMPLATES`, `TERMINAL`, `THEME`, `WORKFLOW`, `DATABASE`, `KV`, `SECURITY`, `FORMATS`, `NOTIFICATIONS`, `AUDIO`, `COLLAB`, `CLI`, `TUI`. Exist so subsystem crates/frontends/tests refer to a plugin without scattering string literals.

## IPC handlers

None — this crate registers no IPC handlers. It is a pure types-and-logic library with no `CorePlugin` impl, no kernel dependency, and no dispatcher. The handlers that *consume* these types live in the service crates (e.g. `bases` helpers are called from `nexus-storage`/`nexus-database` IPC handlers; activity entries are published to the bus by emitter crates).

## Capabilities

None declared, required, or checked here. The crate does not depend on the kernel and has no access to the capability system. It does, however, provide the **mechanism** the kernel uses to enforce path confinement (`ForgePathValidator` / `resolve_within`) — the capability *check* itself happens in kernel/service code that wraps these helpers.

## Settings / Config

None. The crate defines no `Config` struct and persists no settings. The closest thing is the hardcoded `constants` module (IPC timeouts, kernel pool sizes), which are compile-time constants rather than user-configurable settings.

## Events

The crate **publishes and subscribes to nothing itself** (no kernel dependency, no bus access). It *defines* the topic-name constants other crates use:

- `ACTIVITY_APPENDED_TOPIC` = `"com.nexus.activity.appended"` — kernel-owned; every activity emitter publishes `ActivityEntry` here.
- `AI_ACTIVITY_APPENDED_TOPIC` = `"com.nexus.ai.activity_appended"` — legacy AI-only alias; `nexus-ai`'s recorder publishes to both.

The payload type for both topics is `activity::ActivityEntry`.

## Internals & notable implementation details

**`activity` — string-on-the-wire origin.** `ActivityOrigin` is deliberately **not** serde-derived; it is a structured Rust enum that serializes to/from a single flat string via `to_wire`/`from_wire`. The wire form is `kind` or `kind:detail` so the shell can split on the first `:` to bucket entries by `kind()`. `from_wire` is intentionally lossy-tolerant: any unknown prefix (or unknown bare token) round-trips verbatim under `Plugin(...)` so a future emitter (community plugin) never crashes deserialization. `ActivityEntry` carries the origin as a plain `String` field (not the enum) with `#[serde(default = "default_origin_ai")]` so pre-BL-052 JSONL log lines that lack the field still parse, defaulting to `"ai"`. `ActivitySurface` and `ActivityOutcome` use `from_str_lossy`/`Other` for the same forward-compat reason. `deny_unknown_fields` is set on `ActivityEntry` and `ActivityToolCall` to satisfy the audit-2026-05-01 P0-2 schema invariant (every object schema sets `additionalProperties:false`); this rejects *extra* fields, not *missing* ones, so the default-origin behavior is preserved. `truncate_prompt` counts `chars()` (not bytes) and rebuilds the string grapheme-by-grapheme, appending `…`, so multibyte content stays valid UTF-8.

**ts-rs / schemars export.** Only the activity types (`ActivitySurface`, `ActivityOutcome`, `ActivityToolCall`, `ActivityEntry`) carry `#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]` and an `export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"` target. Nothing in `bases`, `obsidian_base`, `paths`, `path_validator`, `constants`, or `plugin_ids` is exported. The `ts-export` feature is **off by default** and `nexus-types` is **not** invoked directly by `scripts/check_ipc_drift.sh`; instead `nexus-bootstrap`'s `ts-export` feature enables `nexus-types/ts-export` (Cargo.toml line 37), and the bootstrap `ipc_schema_emit` test (`cargo test -p nexus-bootstrap --test ipc_schema_emit --features ts-export`) is what regenerates the bindings during the drift check. (Note: `ActivityOrigin` is **not** exported — the shell reads `origin` as a plain string, so the enum has no TS counterpart.)

**Two path validators with different semantics, on purpose.** `path_validator::ForgePathValidator` is the **permissive** validator (strips leading `/`, drops `.`, allows in-root `..`, follows symlinks via `canonicalize`, requires existence) used for user-typed paths that have been through a UI — it backs the kernel context's `confine_path` for `read_file`/`write_file`. `paths::resolve_within` is the **strict** validator (component inspection only, no I/O, `..`/`.`/leading-`/` all rejected) used where `..`-traversal is always wrong (storage IPC handlers, atomic-write targets). The doc comment on `resolve_within` carries a comparison table; the two are not redundant. `validate_for_write` closes the parent-symlink-swap TOCTOU by canonicalizing the deepest existing ancestor and re-joining the non-existing tail, but the residual TOCTOU on the still-non-existing tail components is documented as requiring `openat2(RESOLVE_BENEATH)` at the syscall layer (issue #82). `ForgePathValidator::new` requires the root to already exist on disk because it canonicalizes immediately.

**`bases` — lossy/non-atomic caveats (all tracked under issue #82).** `save_base` is **not crash-atomic across the directory** — each file is a sequential non-atomic `fs::write`, so a crash can leave a new `schema.json` paired with a stale `records.json`; callers needing atomicity should serialize the whole `Base` through the storage plugin's `write_file`. `BaseRecord` flattens user fields with `#[serde(flatten)]` alongside typed `id`/`deletedAt` fields, so a user field literally named `id` or `deletedAt` is silently shadowed on round-trip. `FilterRule::operator` is a free `String` not validated at load time — the canonical operator allowlist is enforced later in `nexus-storage`'s query engine, so a typo only surfaces at query-time. The TOML↔JSON converters are lossy: JSON `null` becomes an empty TOML string, and `i64`-fitting JSON numbers always serialize as TOML integers (`1.0` → `1`). `validate_record` is intentionally narrow — required-field presence only, despite the broad name — and special-cases `"id"` so a missing `id` field never trips the required check.

**`obsidian_base` — round-trip tolerance.** `ObsidianView::view_type` and `FilterNode::Expr` are kept as opaque strings so unknown Obsidian view types and unsupported filter grammar survive a parse→serialize round-trip rather than erroring. `FilterNode` is `#[serde(untagged)]`, so serde discriminates by shape (presence of an `and`/`or`/`not` key vs. a bare string).

**`obsidian_base::filter` — hand-written recursive-descent.** No parser-combinator dep; the grammar is three productions (binary op, method call, negation). The `!` prefix is **only** valid in front of a method call (`!a == b` is rejected — ADR 0019's looser `'!' expr` was deliberately narrowed; issue #82). `find_op_outside_string` walks bytes tracking quote state and honors `\\` / `\"` escapes so an operator-shaped substring inside a string literal (or after a false-closed escaped quote) won't split the expression at the wrong byte. Unsupported expressions are surfaced via `EvalReport::unsupported` (deduplicated) rather than failing silently, so the UI can render a banner. Equality is loose/Obsidian-style: array-valued LHS matches if *any* element matches (so `tags == "book"` works against a tag list); `null` matches only `null`. Comparisons (`>`/`<`/`>=`/`<=`) only apply to number↔number or string↔string and otherwise return `false`.

## Tests

All tests are inline `#[cfg(test)]` modules (no `tests/` directory).

- **`activity.rs`** — origin wire round-trip for all known kinds; unknown-kind fallback to `Plugin`; `kind()` strips detail; legacy JSONL line (no `origin`) parses with origin defaulted to `"ai"`; full entry round-trips with `origin`/`surface`; `ActivitySurface::from_str_lossy` alias normalization; `truncate_prompt` no-op-under-limit and grapheme-boundary-preserving cases (including emoji).
- **`bases.rs`** — parse `schema.json`/`records.json`; parse `views.toml`/`relations.toml` PRD format; full `save → load` round trip; `validate_record` valid + missing-required; `FieldType` kebab-case serde; `load_base` missing-schema error; `init_base` directory creation. Plus a **committed-fixtures guard** (`committed_fixtures_round_trip_through_load_base`) that loads `fixtures/bases/{Tasks,Books,Contacts}.bases` from the repo root (resolved via `CARGO_MANIFEST_DIR`) and validates every record — catches schema/serialization drift before it ships; benignly skips if the fixtures dir is absent (shallow clone).
- **`obsidian_base/mod.rs`** — parses the canonical "reading list" fixture; round-trip preserves structure; empty/missing-filters cases; nested `or/and/not` round-trip; unknown view type round-trips as a string; invalid YAML returns `Parse` error.
- **`obsidian_base/filter.rs`** — two test modules: `find_op_tests` (operator-outside-string detection, including escaped quotes — issue #82 regression) and `tests` (no-filter-matches-all, frontmatter equality/inequality, numeric comparisons, boolean/null literals, file intrinsics, tags-array any-element matching, `contains`/`startsWith`/`endsWith` methods, negated method, quoted-operator-not-split, AND/OR/NOT combination, unsupported-expression recording + dedup, missing-property-is-null).
- **`path_validator.rs`** — nonexistent-root error; canonical-root accessor; valid file/nested resolution; `..` past root rejected vs. in-root `..` allowed; null-byte rejection; absolute-path-treated-as-relative; empty/`.` → root; symlink within root allowed vs. symlink outside root rejected (unix); `validate_for_write` new-file / nested-new-file / traversal-rejected / symlinked-parent-rejected (unix); nonexistent-file error.
- **`paths.rs`** — empty relpath → root; normal join; `..` rejection (including `a/../../outside`); `.` component rejection; absolute-path rejection (unix + windows variants). Also a doctest on `resolve_within`.
