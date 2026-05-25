# nexus-skills

> Kind: lib · IPC plugin id: com.nexus.skills · CorePlugin: yes · Has settings: no (skill files are content, not config) · As of: 2026-05-25

## Overview

A Nexus **skill** is a `.skill.md` file — YAML frontmatter plus a markdown body — that encodes a reusable instruction template the AI engine consumes to shape behaviour for a domain. The frontmatter carries identity and routing metadata (`id`, `name`, `description`, `version`, `author`, `created`, `tags`), auto-activation hints (`applicable_contexts`, `triggers`), a typed `parameters` list, a `depends_on` composition list, and optional `restrictions` / `output_format` / `visibility`. The markdown body is the actual prompt fragment; it may embed `{{ name }}` tokens that get substituted from the declared parameters. Skills are not code — they are content that the inline-AI, Chat panel, and agent planner read and stack into a system prompt. This crate implements PRD-13.

The crate owns three layers: a **parser** (`parse_skill_file` / `parse_skill_text`) that splits the `---` frontmatter block from the body and decodes the YAML into a typed [`Skill`]; an in-memory **registry** ([`SkillRegistry`]) built from a recursive directory walk of `<forge>/.forge/skills/`, keyed by skill `id`; and two consumption helpers — a **substitution** engine ([`render`]) that fills `{{ }}` tokens, and a **composition** resolver ([`compose`], BL-021) that walks the `depends_on` DAG, topologically orders the closure, and merges fragment bodies into one layered prompt.

Discovery is file-as-truth: the `.skill.md` files on disk are authoritative, and the registry is rebuildable from them at any time. A `<root>/REGISTRY.json` index (PRD-13 §3.1) is written alongside as a cold-start optimisation for external CLIs (`load_with_index`); it is never used to short-circuit handler dispatch in the core plugin, and is rejected (forcing a directory walk) if any `.skill.md` is newer than its `last_updated` timestamp or any listed file has vanished. A library of five built-in skills ships in-tree (`builtins/*.skill.md`) and is seeded non-destructively into a fresh forge on bootstrap.

The crate is surfaced to the rest of the system as the `com.nexus.skills` core plugin. It is **load-only** — mutations happen by editing `.skill.md` files on disk and calling `reload`, never by writing through IPC. Agents, the Chat panel, and the future Workflow system consult the catalogue over kernel IPC so no consumer links `nexus-skills` directly. The `invoke` handler closes a deliberate (functional, non-deadlocking) runtime cycle with `com.nexus.agent`: `skills::invoke` composes the skill body and dispatches `com.nexus.agent::session_run`, while the agent planner calls back into `skills::{triggered_by, compose, render}` during planning. Microkernel fit: the kernel never depends on this crate; it reaches storage/AI through the agent it invokes, and all callers route through `ipc_call`.

## Position in the dependency graph

- **Direct nexus-\* deps:** `nexus-kernel` (for `Ipc`, `KernelPluginContext` used by the async `invoke` handler), `nexus-plugins` (for `CorePlugin`, `CorePluginFuture`, `PluginError`, `define_dispatch_helpers!`). Both are upstream of any subsystem.
- **Notable external deps:** `serde` + `serde_yml` (frontmatter decode; `serde_yml::Value` is the in-memory parameter value type), `serde_json` (IPC wire + `REGISTRY.json`), `thiserror`, `tracing`. Optional `ts-rs` + `schemars` behind the `ts-export` feature emit TypeScript + JSON Schema bindings for the IPC arg types. No `chrono`/`time` — RFC3339 timestamps are hand-rolled (Hinnant civil-from-days) in `registry_index`.
- **Crates depending on it:** `nexus-bootstrap` (registers the core plugin and seeds built-ins via `crates/nexus-bootstrap/src/plugins/skills.rs`). No other crate links it directly — `com.nexus.agent` reaches it only over IPC.

## Public API surface

`src/lib.rs` — types and re-exports:
- `Skill` — `{ meta: SkillMeta (flattened), body: String }`. A parsed `.skill.md` entry.
- `SkillMeta` — typed projection of the PRD-13 §2.3 frontmatter schema. Required: `name`, `id`, `description`, `version`, `author`, `created`. Defaulted: `tags`, `applicable_contexts`, `triggers`, `parameters`, `depends_on`, `restrictions`, `output_format`, `visibility`. Unknown keys round-trip through `extra: BTreeMap<String, serde_yml::Value>` for forward-compat.
- `SkillParameter` — one `parameters:` entry: `name`, `type` (renamed `param_type`), optional `description`, `values` (enum allowlist), `items` (list element type), `default`.
- `SkillRestrictions` — capability/tool levers: `modify_files`, `delete_content`, `execute_code` (each `Option<bool>`), `allowed_tools: Vec<String>`. Empty defaults mean "unrestricted". **Advisory metadata only** — not enforced by this crate (see Gaps).

`src/parse.rs` — `parse_skill_text` / `parse_skill_file`, `SkillParseError` (`MissingOpenDelimiter`, `MissingCloseDelimiter`, `InvalidYaml`, `Io`). Splits the leading `---` … `---` block from the body; handles a BOM, leading whitespace, and CRLF; preserves the body verbatim.

`src/registry.rs` — `SkillRegistry`, `SkillRegistryError` (`Io`, `PartialParseFailure { count, first }`). Methods: `empty`, `load` (recursive walk, rejects duplicate ids), `load_with_index` (cold-start index fast-path), `len`/`is_empty`, `get`, `path_for`, `iter`, `entries`, `by_context` (filter on `applicable_contexts`), `triggered_by` (case-insensitive substring match on `triggers`), `insert`, `remove`. Symlinks are skipped during the walk (issue #85, prevents escaping the root).

`src/registry_index.rs` — `RegistryIndex`, `RegistryIndexEntry`, `RegistryIndexError`, `REGISTRY_INDEX_VERSION = "1.0"`, `write_index` (atomic via `*.json.tmp` + rename), `read_index`. Persistent JSON projection of the registry with forward-slash root-relative paths.

`src/substitute.rs` — `render`, `SubstitutionError` (`MissingParameter`, `EnumMismatch`). Replaces `{{ name }}` tokens for declared parameters; undeclared tokens pass through; tokens don't cross a newline.

`src/compose.rs` — `compose`, `ComposedSkill`, `ComposedFragment`, `ComposeConflict` (`ParameterClash`, `RestrictionsDisagree`), `ComposeError` (`UnknownRoot`, `MissingDependency`, `Cycle`). DFS topological resolution of the `depends_on` closure with cycle detection.

`src/builtins.rs` — `seed_builtins` (idempotent, non-destructive write of in-tree skills into a dir), `builtin_filenames`, `SeedReport { created, skipped }`.

`src/core_plugin.rs` — `SkillsCorePlugin`, `PLUGIN_ID`, the `HANDLER_*` ids, and the typed arg structs (`GetSkillArgs`, `ListByContextArgs`, `TriggeredByArgs`, `RenderSkillArgs`, `ComposeSkillArgs`, `InvokeSkillArgs`).

## IPC handlers

Registered with `LifecycleFlags::NONE`; handler ids are append-only and listed in `IPC_HANDLERS` (SD-06 single source of truth). The bootstrap also registers `v1` command aliases.

| Id | Command | Args | Returns | Capability | Description |
|---:|---------|------|---------|------------|-------------|
| 1 | `list` | `{}` | JSON array of `Skill` objects, each augmented with a `relpath` field (forge-relative `.forge/skills/...` path, BL-022) | none | Every loaded skill, id-sorted. |
| 2 | `get` | `GetSkillArgs { id }` | `Skill` object + `relpath` | none | One skill by id; `ExecutionFailed` ("no skill with id …") if absent. |
| 3 | `list_by_context` | `ListByContextArgs { context }` | JSON array of `Skill` | none | Skills whose `applicable_contexts` contains `context`. |
| 4 | `triggered_by` | `TriggeredByArgs { text }` | JSON array of `Skill` | none | Skills with a non-empty `triggers` phrase appearing (case-insensitive) in `text`. |
| 5 | `reload` | `{}` | `{ "loaded": <count> }` | none | Re-scans `<forge>/.forge/skills`, replaces the registry with the parsed subset, refreshes `REGISTRY.json`. |
| 6 | `render` | `RenderSkillArgs { id, values? }` | `{ id, name, body }` (body with `{{ }}` substituted) | none | Renders a skill body; `values` JSON is round-tripped to YAML so enum comparison matches the declared `values:`. |
| 7 | `compose` | `ComposeSkillArgs { id }` | `ComposedSkill { root_id, fragments[], merged_body, conflicts[] }` | none | BL-021 — resolves the `depends_on` closure into ordered fragments + merged body; cycle/missing-dep surface as `ExecutionFailed`. |
| 8 | `invoke` | `InvokeSkillArgs { skill_id, input, archetype? }` | the agent observation JSON from `session_run`, verbatim | none (downstream gating only) | BL-054 Phase 3 — composes the skill's merged body as system prompt, then dispatches `com.nexus.agent::session_run` with `goal = input`, `archetype = archetype ?? "general"`, `auto_approve = true`, 120s outer cap. **Async-only**: a synchronous `dispatch` of id 8 returns `PluginError::HandlerIsAsyncOnly`. |

## Capabilities

**None declared or checked.** The manifest is built via `core_manifest_with_ipc` with no capability list, and no handler calls a capability gate. The catalogue/templating handlers are pure and side-effect-free. `invoke` itself declares no capability — any real privilege gating happens **downstream**: the storage/AI verbs the invoked agent ultimately calls are capability-checked in their own service crates. The `SkillRestrictions` frontmatter block (`modify_files`, `execute_code`, `allowed_tools`, …) is parsed and surfaced in `compose` conflicts but is **not** enforced as a runtime capability by this crate (see Gaps). This matches `docs/0.1.2/ipc-handlers.md`, which lists every `com.nexus.skills` command with caps "—".

## Settings / Config

No `Config` struct and no `.forge/*.toml` file — skills are content, not configuration. The relevant format spec is the `.skill.md` frontmatter schema (PRD-13 §2.3), projected by `SkillMeta`:

| Field | Type | Required | Default | Notes |
|-------|------|----------|---------|-------|
| `name` | string | yes | — | Human-readable display name. |
| `id` | string | yes | — | Unique kebab-case identifier; registry key. Duplicate ids across files are rejected as a parse failure. |
| `description` | string | yes | — | One-to-two-sentence purpose. |
| `version` | string | yes | — | Semantic version. |
| `author` | string | yes | — | Author or org. |
| `created` | string | yes | — | ISO 8601 creation date. |
| `tags` | list\<string\> | no | `[]` | Discovery tags. |
| `applicable_contexts` | list\<string\> | no | `[]` | Auto-activation contexts: `pull-request`, `terminal`, `editor`, `ai-chat`, `agent`. |
| `triggers` | list\<string\> | no | `[]` | Keyword/phrase triggers (case-insensitive substring). |
| `parameters` | list\<SkillParameter\> | no | `[]` | Typed inputs for `{{ }}` substitution. |
| `depends_on` | list\<string\> | no | `[]` | Other skill ids this layers on (composed root-last). |
| `restrictions` | object | no | none | `modify_files`/`delete_content`/`execute_code` (bool) + `allowed_tools` (list). Advisory. |
| `output_format` | string | no | none | `structured` / `markdown` / `natural` / `custom`. |
| `visibility` | string | no | none in `SkillMeta`; `"public"` default in the `REGISTRY.json` entry | `public` (shareable) or `private`. |

`SkillParameter`: `name`, `type` (`enum`/`list`/`string`/`number`/`boolean`/custom), optional `description`, `values` (enum allowlist), `items` (list element type), `default`. Unknown frontmatter keys are preserved in `SkillMeta::extra`.

`REGISTRY.json` is a derived index, not user config: `{ version: "1.0", last_updated: <RFC3339 UTC>, skills: [ { id, name, path, version, tags, applicable_contexts, author, visibility } ] }`.

Five built-in skills ship in-tree and seed on bootstrap: `code-reviewer` (`builtin.code-reviewer`), `daily-journal`, `meeting-notes`, `commit-message`, and `os-setup` (BL-054 Phase 5 — agentic-OS architecture elicitation interview, run via the SkillsPanel Run button). Seeding is non-destructive — a file already present at the target path is skipped, so users can shadow a built-in.

## Events

None. The crate neither publishes nor subscribes to the kernel event bus. (The `EventBus` reference passed to `register` in bootstrap is only used by `or_lifecycle_skip` for lifecycle reporting, not by this crate.)

## Internals & notable implementation details

- **Parsing** (`split_frontmatter`): strips a leading BOM, skips leading whitespace, then requires the first non-empty content to be a `---` line (accepting `---\r\n`, `---\n`, or bare `---`). The close is the next line that trims to exactly `---`. The body is everything after the closing delimiter, verbatim. YAML decodes into `SkillMeta` with `#[serde(flatten)] extra` capturing unmodeled keys.
- **Registry** is a `BTreeMap<String, RegistryEntry { path, skill }>` keyed by id (so `iter`/`entries` are id-sorted). `load` walks recursively, filters strictly on the `.skill.md` suffix, skips symlinks (issue #85 — a symlink to `/etc` could otherwise smuggle the walker outside the root), and records both parse errors and duplicate-id collisions as `PartialParseFailure`. Crucially, `load` populates the registry with the successfully-parsed subset **even when it returns `Err`** — the core plugin's `open`/`reload` exploit this by logging the warning and keeping the parsed subset.
- **Cold-start index** (`load_with_index` → `try_load_from_index`): reads `REGISTRY.json`, parses its `last_updated` into epoch seconds, walks the tree for any `.skill.md` newer than that timestamp, and rejects the index (falling back to a full walk) if anything is newer, any listed path is missing, or any listed file's frontmatter `id` no longer matches its index entry. The RFC3339 ↔ epoch conversion is hand-rolled both directions (Hinnant civil-from-days) to avoid a `chrono`/`time` dependency; `parse_rfc3339_seconds` is strict about the `YYYY-MM-DDTHH:MM:SSZ` 20-char shape.
- **Index writes are atomic**: serialize → write `*.json.tmp` → rename over destination; on any error the destination is untouched and the tmp file is best-effort removed.
- **Core plugin lifecycle**: `open` eagerly loads the registry (downgrading any load error to a `warn` + empty/partial registry) and best-effort writes `REGISTRY.json`. `reload` re-runs `load`, and on `PartialParseFailure` re-runs `load` once more into a fresh registry to keep the clean subset, then refreshes the index. The registry sits behind a `Mutex` so dispatch is `Send + Sync`; a poisoned mutex maps to an `ExecutionFailed`.
- **`invoke` async plumbing** (BL-054 Phase 3): synchronous `dispatch` of `HANDLER_INVOKE` returns `HandlerIsAsyncOnly`; the real work runs in `dispatch_async`. `compose_for_invoke` parses args, validates non-empty `skill_id`, confirms the skill exists, and composes the merged body **synchronously** (so the `Mutex` guard never crosses an `.await`, keeping the future `Send`). The future then calls `com.nexus.agent::session_run` via the captured `KernelPluginContext` (wired by `wire_context`); a missing context yields a clear "no kernel context wired" error rather than a panic. The intentional runtime cycle with `com.nexus.agent` is documented in both crates' `core_plugin.rs`.
- **Substitution** (`render`): resolves each declared parameter from supplied values then `default`; a declared parameter with neither errors (`MissingParameter`); enum parameters with a non-empty `values:` reject out-of-range input (`EnumMismatch`). Tokens are byte-scanned for `{{ … }}`; `find_close` refuses to span a newline; only declared names are substituted (undeclared `{{ foo }}` passes through). Values are stringified from `serde_yml::Value` (enum equality compares stringified forms).
- **Composition** (`compose`): recursive DFS with white/gray/black colouring; a back-edge into a gray node yields `Cycle` with the offending path (back-edge target repeated at the end). Post-order yields deps-first / root-last ordering, deterministic within a layer because children are visited in `depends_on` declaration order; diamonds dedupe to a single visit. `merged_body` joins fragments under `## Skill: <name> [<id>]` headings. `detect_conflicts` is non-fatal and advisory: `ParameterClash` when one parameter name has divergent `(type, default)` across the closure; `RestrictionsDisagree` per-lever when boolean levers diverge or non-empty `allowed_tools` sets differ.

## Tests

All tests are inline `#[cfg(test)]` modules (no `tests/` directory). `tempfile` and `tokio` (current-thread, for the async `invoke` tests) are dev-deps.

- `parse.rs` — minimal valid skill, unknown-key preservation in `extra`, missing open/close delimiter rejection, parameters-list decode (enum + list), CRLF handling.
- `registry.rs` — recursive walk reads every `.skill.md` (ignores non-skill files), partial parse-failure surfacing, duplicate-id rejection, missing-root → empty, `load_with_index` uses the index when fresh / falls back when missing / falls back when a listed path vanished, `by_context` + `triggered_by` filtering.
- `registry_index.rs` — write→read round-trip of two skills (incl. default `visibility`), forward-slash paths on all platforms, atomic-write leaves no partial/tmp file on error, missing-file → `Io(NotFound)`, version pinned to `"1.0"`, RFC3339 shape, epoch-zero and a known timestamp format check.
- `substitute.rs` — supplied value, default fallback, missing-parameter error, enum reject/accept, undeclared pass-through, whitespace-less tokens, no newline crossing.
- `compose.rs` — self-only when no deps, deps-before-dependents ordering, diamond dedupe, cycle (full path + self-cycle), missing-dependency, unknown-root, parameter-clash and restriction-disagreement conflict surfacing.
- `builtins.rs` — every built-in parses, seed creates all on a fresh dir, seed is idempotent, seed does not overwrite user edits.
- `core_plugin.rs` — `list`/`get`/`list_by_context`/`triggered_by` dispatch, `render` substitution (supplied + default), `render` unknown-skill error, `open` writes `REGISTRY.json`, `reload` rewrites the index and picks up new files, and the `invoke` routing set (sync → `HandlerIsAsyncOnly`, async without context, async unknown-skill short-circuit, async returns `None` for unrelated handlers).
