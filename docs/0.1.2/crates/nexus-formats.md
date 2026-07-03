# nexus-formats

> Kind: lib · IPC plugin id: com.nexus.formats · CorePlugin: yes · Has settings: AppConfig + WorkspaceState + AiConfig · As of: 2026-05-25

## Overview

`nexus-formats` is the pure-logic file-format library for Nexus (PRD 06). It owns the parsers and serializers for every text format a forge stores on disk — CommonMark + GFM + Nexus-extension markdown (`.md`/`.mdx`), the Obsidian-compatible JSON canvas format (`.canvas`), and the four forge configuration files (`app.toml`, `workspace.json`, `mcp.toml`, `ai.toml`). It also carries the importer/exporter for Notion "Export → Markdown & CSV" zip archives, the CSV ⇄ `.bases` (TOML) bridge those imports use, the frontmatter-version migration runner, and a grab-bag of filename / slug / MIME / SHA-256 / attachment-naming utilities.

The crate is deliberately **SQL-free and service-free**: every public function is a pure transformation over strings, paths, or byte buffers (the only side effects are direct filesystem reads/writes in the config loaders and the Notion zip walker). It holds no database handle, spawns no threads, and subscribes to no events. This is by design — under the file-as-truth invariant, the SQLite index and Tantivy FTS index in `.forge/` are *derived* from the markdown/canvas files, so the code that parses those files must be reusable from storage's indexer, from the editor, from the CLI, and from the shell without dragging a runtime along. Bases *types* themselves live in `nexus_types::bases`, not here; the active runtime consumers (database/CLI/storage) build on that type hierarchy, and `nexus-formats` only touches bases as a serialization target during Notion conversion.

Within the microkernel architecture the library half is consumed directly by leaf crates that already sit above it (storage, editor, crdt, cli), while the *forge-mutating* surface — Notion import/export plus single-note HTML export, all of which walk/write the filesystem — is exposed through a thin `CorePlugin` (`com.nexus.formats`) with three IPC handlers. That keeps the file-walking, path-resolution side effects on one IPC path reachable uniformly from CLI, TUI, MCP, and the shell, rather than scattered as direct calls. Everything else (parse a markdown string, load `app.toml`, slugify a title) is a plain function call against the library.

The crate's `lib.rs` enforces `#![deny(missing_docs)]` and `#![warn(clippy::pedantic)]`.

## Position in the dependency graph

- **Direct nexus-\* deps:** only `nexus-plugins` (for the `CorePlugin` / `PluginError` traits and the `define_dispatch_helpers!` macro). No dependency on `nexus-kernel`, `nexus-types`, or any service crate — it is effectively a leaf above `nexus-plugins`.
- **Notable external deps (+why):**
  - `comrak` — CommonMark + GFM AST parsing and HTML rendering (markdown pipeline, HTML export).
  - `serde` / `serde_json` / `serde_yml` / `toml` — config (de)serialization, canvas JSON, YAML frontmatter, `.bases` TOML.
  - `csv` — Notion database CSV read/write.
  - `zip` — reading Notion export archives.
  - `sha2` — content hashing (`sha256_hex`, attachment names, parse `content_hash`).
  - `regex-lite` — `${ENV_VAR}` placeholder matching in config substitution (lightweight, no full `regex`).
  - `uuid` — UUIDv7 generation for Notion export filenames.
  - `chrono` — pulled in for date handling (declared dep; dates are otherwise kept as raw strings).
  - `thiserror` — error enums.
  - `tracing` — declared dep (logging hooks).
  - `ts-rs` + `schemars` — optional, behind the `ts-export` feature; derive TS bindings + JSON schema for the two IPC arg structs.
- **Crates depending on it:** `nexus-bootstrap` (registers the core plugin), `nexus-storage`, `nexus-editor`, `nexus-crdt`, `nexus-cli`.

## Public API surface

### `error` (re-exported at crate root)
- `Error` — top-level enum wrapping `Markdown`/`Canvas`/`Config`/`Util` sub-errors plus `Io`, via `#[from]`.
- `MarkdownError` — `FrontmatterParse`, `EmbedDepthExceeded`, `CircularEmbed`.
- `CanvasError` — `InvalidJson`, `MissingVersion`.
- `ConfigError` — `TomlParse`, `JsonParse`, `UndefinedEnvVar`.
- `UtilError` — `InvalidFilename`, `PathTooLong`.
- `Result<T>` — alias for `std::result::Result<T, Error>`.

### `markdown`
- `parse(content: &str) -> Result<ParsedMarkdown, MarkdownError>` — full parse: frontmatter, blocks, links, tags, tasks, math, content hash.
- `parse_frontmatter(path: &Path) -> Result<HashMap<String, serde_json::Value>, MarkdownError>` — cheap frontmatter-only read from a file (reserved keys + custom merged into one JSON map). Used for sidebar/search indexing.
- `resolve_wikilink(target, source_dir, forge_root) -> Option<PathBuf>` — relative-then-stem resolution.
- `export_to_html(content, title) -> String` — standalone styled HTML doc (safe mode, raw HTML escaped); C67 (#420) rendering of Nexus's own conventions (frontmatter stripped, wikilinks/known-callout-kinds/image-embeds rendered, not left literal).
- Types: `ParsedMarkdown`, `Block`, `BlockKind` (Heading/Paragraph/CodeBlock/List/Table/Callout/BlockQuote), `Task`, `Tag`, `TagSource` (Frontmatter/Inline), `MathSpan`, `Frontmatter`, `WikiLink`, `LinkType` (Wikilink/Embed/Markdown).
- Sub-module functions: `frontmatter::extract`, `frontmatter::MAX_FRONTMATTER_BYTES`; `extensions::{detect_callout, extract_block_ref, extract_inline_tags, extract_math_spans}`; `wikilinks::scan`; `embed::{resolve_embeds, MAX_EMBED_DEPTH}`.

### `canvas`
- `parse(json) -> Result<CanvasFile, CanvasError>` / `parse_with_path(json, path)` — parse with size + element-count caps.
- `serialize(canvas) -> Result<String, CanvasError>` — pretty JSON.
- `file_links(canvas) -> Vec<String>` — vault-relative paths of `file` nodes.
- `MAX_CANVAS_BYTES` (50 MiB), `MAX_CANVAS_ELEMENTS` (100 000).
- Types: `CanvasFile`, `CanvasNode`, `CanvasNodeType` (File/Text/Link/Group/Database/Terminal), `CanvasEdge`, `CanvasEdgeType` (Solid/Dashed/Dotted), `CanvasBackground`.

### `config`
- Loaders/savers: `load_app_config`/`save_app_config`, `load_workspace_state`/`save_workspace_state`, `load_mcp_config`/`save_mcp_config`, `load_ai_config`/`save_ai_config`. All take `forge_root: &Path`; loaders return defaults when the file is absent and run `${ENV_VAR}` substitution before parse.
- Types: `AppConfig` (+ `CoreSettings`, `EditorSettings`, `PreviewSettings`, `SearchSettings`, `PluginSettings`, `GitSettings`, `DreamCycleSettings`), `WorkspaceState` (+ `OpenFileEntry`, `PanelLayout`, `PanelConfig`), `McpConfig` (+ `McpServerEntry`), `AiConfig` (+ `AiProvider`, `AiModel`).
- `env_subst::substitute(text) -> Result<String, ConfigError>` (public module).

### `util`
- `slugify(input) -> String` — URL-safe slug (lowercase, hyphen-collapse, ASCII-only).
- `validate_filename(name) -> Result<(), UtilError>` / `validate_path(path) -> Result<(), UtilError>`; consts `MAX_FILENAME_BYTES` (255), `MAX_PATH_BYTES` (260).
- `detect_mime(ext) -> &'static str` — extension → MIME, defaulting to `application/octet-stream`.
- `sha256_hex(data) -> String` — 64-char lowercase hex digest.
- `attachment_name(file_type, timestamp_ms, content, ext) -> String` — deterministic `{type}-{ts}-{hash8}.{ext}`.

### `migration`
- `detect_version(content) -> Result<FormatVersion, MigrationError>` — reads `version:` frontmatter, defaults to v1.0.
- `scan_versions(forge_root) -> io::Result<Vec<VersionTally>>` — walk `.md` files, tally versions (skips hidden dirs).
- `FormatVersion` (`parse`, `new`, `to_string_compact`), `MigrationRegistry` (`register`/`migrate`/`pairs`/`len`/`is_empty`), `MigrationFn`, `MigrationError`, `VersionTally`, `DEFAULT_VERSION` ("1.0"). The registry ships empty — no breaking forge-format change exists yet; the infra is staged for the first v2.0.

### `notion`
- `import_notion_zip(zip_path, dest) -> Result<ImportReport>` / `import_notion_archive<R: Read+Seek>(reader, dest)`.
- `export_to_notion(source, dest) -> Result<ExportReport>`; `export::bases_to_csv(toml_body) -> Result<String>`.
- `database::csv_to_bases(csv_str, name) -> Result<String>`.
- `filename::{strip_notion_uuid, extract_uuid, clean_path}`.
- `property::extract_property_table(input) -> (Option<BTreeMap>, String)`.
- `markdown::{convert_notion_markdown, has_unconverted_warning_marker}`.
- Types: `ImportReport`, `ExportReport`.

### `core_plugin`
- `FormatsCorePlugin::open(forge_root) -> Self`; implements `CorePlugin`.
- Consts: `PLUGIN_ID`, `HANDLER_IMPORT_NOTION` (1), `HANDLER_EXPORT_NOTION` (2), `HANDLER_EXPORT_HTML` (3, C66 #419), `IPC_HANDLERS` (the `(command, id)` pairs consumed by bootstrap).
- Arg structs: `ImportNotionArgs { source: PathBuf, dest: Option<PathBuf> }`, `ExportNotionArgs { source: Option<PathBuf>, dest: PathBuf }`, `ExportHtmlArgs { source: PathBuf, title: Option<String>, dest: Option<PathBuf> }` (all `#[serde(deny_unknown_fields)]`, TS/schema-exported under `ts-export`).

## IPC handlers

| command | args | returns | capability | description |
|---------|------|---------|------------|-------------|
| `import_notion` (handler 1) | `{ source: PathBuf, dest?: PathBuf }` | `{ pages_written, bases_written, attachments_copied, warnings: [string], dest: string }` | none enforced (core trust) | Import a Notion "Markdown & CSV" zip export. `source` must exist (else error). `dest` is forge-relative if not absolute; defaults to `<forge>/Imported from Notion`. Delegates to `notion::import_notion_zip`. |
| `export_notion` (handler 2) | `{ source?: PathBuf, dest: PathBuf }` | `{ pages_written, databases_written, attachments_copied, warnings: [string], dest: string }` | none enforced (core trust) | Export a forge subdirectory to a Notion-compatible folder tree. `source` is forge-relative if not absolute and defaults to the whole forge root; must be a directory (else error). Delegates to `notion::export_to_notion`. |
| `export_html` (handler 3) | `{ source: PathBuf, title?: String, dest?: PathBuf }` | `{ html: string }` when `dest` is omitted, else `{ written: true, dest: string }` | none enforced (core trust) | C66 (#419) — render a single forge note to a standalone styled HTML document via `markdown::export_to_html`. `source` is forge-relative if not absolute; read failures error. `title` defaults to `source`'s file stem. When `dest` is given (forge-relative if not absolute) the HTML is written there (parent dirs created) instead of returned inline. Reachable from the CLI (`nexus content export`, via `nexus_bootstrap::export_to_html` calling the library function directly rather than this handler), the MCP `nexus_export_html` tool, and the shell's editor "Export as HTML" command. |

All three handlers are synchronous/blocking (they walk the filesystem and parse files); the kernel runs each dispatch on a dedicated thread. Handler ids are append-only. Bootstrap also registers `.v1` aliases for each command via `with_v1_aliases` (ADR 0021). Unknown handler ids return an execution error.

## Capabilities

**None declared or enforced by this crate.** The bootstrap manifest (`core_manifest_with_ipc("com.nexus.formats", …)`) emits `trust_level = "core"` with `[[registrations.ipc_command]]` blocks but no `[[capabilities]]` section and `LifecycleFlags::NONE`. As a core plugin it runs with full host access; the file reads/writes performed by the Notion handlers happen directly via `std::fs` inside the library, not through a capability-gated `fs.read`/`fs.write` kernel mediation. The `ipc-handlers.md` reference notes for this plugin: "pure parse / serialize; fs ops route through storage" — that describes the intended boundary for shell-driven flows, but the `import_notion`/`export_notion` handlers themselves touch the filesystem directly within the plugin's forge root.

## Settings / Config

All four config files live under `<forge>/.forge/`. Loaders return `T::default()` when the file is missing and run `${ENV_VAR}` substitution before deserialization; `#[serde(default)]` on the structs means partial files merge over defaults. Savers `create_dir_all(.forge)` then write pretty-printed TOML/JSON.

### `AppConfig` → `app.toml` (TOML)
Top-level `#[serde(default)]`. Sections:
- `core: CoreSettings` — `name` ("MyForge"), `default_note_dir` ("notes"), `attachment_dir` ("attachments"), `daily_note_format` ("%Y-%m-%d"), `default_layout` ("sidebar"), `theme` ("auto"), `language` ("en").
- `editor: EditorSettings` — `font_size` (14), `font_family` ("monospace"), `line_height` (1.6), `enable_vim_mode` (false), `auto_save` (true), `auto_save_delay_ms` (3000).
- `preview: PreviewSettings` — `enable_mermaid`/`enable_katex`/`enable_highlight`/`enable_wikilinks` (all true).
- `search: SearchSettings` — `enable_full_text` (true), `index_interval_ms` (5000), `max_results` (50).
- `plugins: PluginSettings` — `enabled: Vec<String>` (empty).
- `git: GitSettings` — `enabled` (true), `auto_commit` (false), `auto_commit_interval_secs` (1800), `auto_commit_on_save` (false), `auto_commit_debounce_secs` (5), `poll_interval_secs` (Option, skip-if-none → git crate default 2 s), `auto_commit_tick_secs` (Option → git default 30 s).
- `dream_cycle: DreamCycleSettings` (BL-129) — `enabled` (false), `schedule` ("0 2 * * *"), `merge_threshold` (0.97), `review_threshold` (0.92), `decay_factor` (0.95), `decay_floor` (0.10); C44 (#397) extraction sub-settings — `extract_enabled` (false, opt-in even when `enabled` is true), `extract_lookback_hours` (24), `extract_max_notes_per_cycle` (10), `extract_max_entities_per_note` (3).
- `settings: BTreeMap<String, toml::Value>` — flat `pluginId.fieldName` bag mirrored by the shell's settings registry; `BTreeMap` keeps on-disk order stable.

### `WorkspaceState` → `workspace.json` (JSON)
`#[serde(default)]`. Fields: `active_file: Option<String>` (None), `open_files: Vec<OpenFileEntry>` (each `{ file, line, column }`), `sidebar_collapsed` (false), `panel_layout: PanelLayout` (`left` = 250px/open, `right` = 300px/collapsed; `PanelConfig` = `{ width, collapsed }`), `recent_files: Vec<String>`, `search_query` (""), `theme` ("dark").

### `AiConfig` → `ai.toml` (TOML)
`#[serde(default)]`. Consts: `DEFAULT_PROVIDER` ("anthropic"), `DEFAULT_MODEL` ("claude-sonnet-4-6"), `DEFAULT_API_KEY_ENV` ("ANTHROPIC_API_KEY"), `DEFAULT_MAX_TOKENS` (4096), `DEFAULT_TEMPERATURE` (0.7). Fields: `provider`, `model`, `api_key_env: Option<String>` (default `Some("ANTHROPIC_API_KEY")`), `embedding_model: Option`, `max_tokens` (4096), `temperature` (0.7), then a block of P2-04/05/06 per-provider override `Option`s (all default `None`): `anthropic_model`, `openai_chat_model`, `openai_embedding_model`, `ollama_chat_model`, `ollama_base_url`, `ollama_embedding_model`, `ollama_temperature: Option<f32>`, `indexing_debounce_secs: Option<u64>`; plus `providers: BTreeMap<String, AiProvider>` (`{ type, apiKey?, baseUrl? }`) and `models: Vec<AiModel>` (`{ id, provider, max_tokens=4096, temperature=0.7, systemPrompt? }`). `apiKey` values may carry `${ENV_VAR}` placeholders resolved at load.

### `McpConfig` → `mcp.toml` (TOML)
Not in the task's "has settings" list but loaded/saved by this crate. `#[serde(default)]`: `enabled` (true), `transport` ("stdio"), `allowed_tools: Vec<String>`, `mcp: BTreeMap<String, McpServerEntry>` (`{ type, command?, args, url?, apiKey?, timeout?, env }`).

Per the settings-promotion guardrail, new fields belong in the owning service crate's `Config` struct, not new top-level `.forge/` files.

## Events

**None.** The crate publishes and subscribes to no kernel events; it has no `EventBus` handle. The two IPC handlers are request/response only.

## Internals & notable implementation details

**Markdown pipeline (`markdown/`).** `parse` first strips YAML frontmatter, seeds the tag list from `frontmatter.tags`, then runs comrak (`parse_document`) with `strikethrough`, `table`, `autolink`, and `tasklist` extensions enabled. It walks only *top-level* AST children, emitting a `Block` per heading/paragraph/code/list/table/blockquote and harvesting wikilinks, inline tags, math spans, block-ref anchors, and tasks along the way. Blockquotes are classified Callout vs BlockQuote via `detect_callout` (`[!TYPE]` prefix, lowercased). `content_hash` is the SHA-256 of the raw bytes. Tags are deduped by `(name, source)`. Inline-tag scanning requires the `#` to be at string start or after whitespace (so URL anchors aren't tags). Math extraction does block `$$…$$` first, then inline `$…$` with no-leading/trailing-space guards, tracking consumed ranges to avoid double-matching. Wikilink scanning is a manual byte scan (detects the preceding `!` for embeds; splits `target#fragment` and `target|display`).

**Frontmatter (`frontmatter.rs`).** Requires a literal `---\n` opener and a `\n---` closer; unterminated blocks are treated as no-frontmatter (full body returned). Reserved keys (`title`, `type`→`doc_type`, `status`, `cssclass`, `date`, `created`, `modified`, `version`, `aliases`, `tags`) map to typed fields; everything else lands in `custom: HashMap<String, serde_json::Value>`. **Issue #78 hardening:** a `MAX_FRONTMATTER_BYTES` (256 KiB) cap fires *before* `serde_yml` runs, short-circuiting billion-laughs-shaped YAML.

**Embed resolution (`embed.rs`).** `resolve_embeds` takes an injected `reader` closure (testability) and recursively substitutes `![[target]]` with resolved content, guarding against `MAX_EMBED_DEPTH` (10) and cycles (best-effort canonicalized path set). Targets resolve `parent_dir/target` then `forge_root/target`.

**HTML export (`html.rs`).** Renders with the same comrak extensions but `render.unsafe = false` (raw HTML escaped). Wraps the body in a complete standalone document with an inlined GitHub-ish stylesheet; the `<title>` is manually HTML-escaped. C67 (#420) — three additional comrak extensions turn Nexus's own conventions into real markup instead of leaking/staying literal: `front_matter_delimiter` strips leading YAML frontmatter from the output; `wikilinks_title_after_pipe` renders `[[target|display]]` (target-first, matching Nexus's own pipe order) as a real `<a data-wikilink="true">`, unresolved to an actual forge path (needs forge/source-dir context this pure function doesn't have — a real fix is a separate follow-up); `alerts` renders GitHub's 5-keyword callout set (`note`/`tip`/`important`/`warning`/`caution`) as styled `.markdown-alert` divs — Nexus's own callout convention accepts any alphabetic type and the shell recognizes 12 kinds, so the other 7 fall through to a plain blockquote exactly as before this fix (documented partial coverage, not a regression). A pre-parse `rewrite_image_embeds` pass additionally turns `![[image.png]]`/`![[image.png|caption]]` into standard Markdown image syntax so comrak's native image support renders a real `<img>`; non-image embeds (`![[another-note]]`) are unaffected — comrak doesn't recognize `[[...]]` preceded by `!` as a wikilink at all. Relative image/asset resolution and inlining remain out of scope (no existing precedent in the codebase; a real feature on its own).

**Canvas (`canvas/`).** `CanvasFile` mirrors the Obsidian/JSON Canvas 1.0 schema with `#[serde(flatten)] extra` catch-alls on file/node/edge for forward-compat round-tripping (e.g. Obsidian's `subpath`, `styleAttributes`, `fromSide`/`toSide`). `version` defaults to "1.0" when absent. Edges accept both spec `fromNode`/`toNode` and legacy `from`/`to` aliases on read, serializing as `fromNode`/`toNode`; `edge_type` defaults to Solid. `CanvasBackground` (`color` + optional `pattern`) is a Nexus extension over the 1.0 spec. **Issue #78 hardening:** `MAX_CANVAS_BYTES` (50 MiB) checked before parse, and `MAX_CANVAS_ELEMENTS` (100 000 nodes+edges) checked after, to bound malicious inputs.

**Config (`config/`).** Generic `load_toml`/`save_toml`/`load_json`/`save_json` helpers under `.forge/<file>`. `env_subst::substitute` uses a `OnceLock<Regex>` for `\$\{([A-Za-z_][A-Za-z0-9_]*)\}` and errors on the *first* undefined variable (`UndefinedEnvVar`). No format migration exists for these files; partial files merge over `Default` via `#[serde(default)]`.

**Notion import (`notion/`).** Two-pass over the zip: pass 1 builds a `LinkIndex` mapping every entry to a cleaned destination path and every URL-encoded `.md` filename to its display title (for mention-link rewriting); a common top-level folder (Notion wraps exports in one) is detected and stripped. Pass 2 reads each entry: `.md` → `convert_page` (extract 2-column property table after H1 → frontmatter, rewrite `[Title](Enc%20uuid.md)` → `[[Title]]`, convert leading-emoji blockquotes → `[!type]` callouts, inject `notion_id` + `source: notion` frontmatter); `.csv` → `csv_to_bases` writing a sibling `.bases` *and* keeping the raw CSV as an attachment; everything else copied verbatim. Filename collisions get a ` (n)` numeric suffix via `unique_path`. UUID handling (`filename.rs`): a Notion suffix is exactly 32 lowercase hex chars preceded by a single space; uppercase or wrong-length suffixes are rejected. `csv_to_bases` (`database.rs`) infers one type per column from up to 256 sampled rows via a `bool → number → date → string` cascade (empty cells never disqualify), emitting `[[fields]]`, a default `[[views]]` table view, and inline `[[records]]` with TOML-escaped keys/values.

**Notion export (`export.rs`).** Inverse: pass 1 indexes title → `Title <uuid>.md` (uuid from `notion_id` frontmatter or a fresh UUIDv7 via `uuid::now_v7().simple()`); pass 2 writes `# Title` + a property table built from non-`notion_id`/non-`source` frontmatter + body with callouts emoji-ized (`[!note]`→💡, `[!warning]`→⚠️, etc.) and `[[wikilinks]]` rewritten to `[Title](Title%20uuid.md)` mention links (unresolved targets warn but pass through). `.bases` files convert back to CSV via `bases_to_csv`; other files copy verbatim; dotfiles/`.forge` are skipped. Both passes use a hand-rolled `parse_frontmatter` (distinct from the library's YAML parser — simple `key: value`, dequotes `"…"`) and a defensive `utf8_char_len` byte walker for link rewriting.

## Tests

Comprehensive in-module `#[cfg(test)]` coverage plus one integration test file:

- `error.rs` — display formatting + `#[from]` wrapping for each sub-error.
- `migration.rs` — version parse/round-trip/ordering, `detect_version` defaults & errors, registry dispatch/no-migration/error surfacing, `scan_versions` (skips hidden dirs, records "unknown").
- `markdown/mod.rs` — headings, paragraphs, code, frontmatter, inline+frontmatter tags, wikilinks (display/fragment), embeds, tables, lists, block refs, callouts, tasks, content-hash shape.
- `markdown/frontmatter.rs` — reserved/custom keys, tags/aliases lists, unterminated/malformed handling, date passthrough.
- `markdown/extensions.rs` — inline tags (incl. URL-anchor negative case, dedup), callouts (case-insensitive), block refs (end-only), inline/block math (incl. `$5` negative case).
- `markdown/wikilinks.rs` — plain/display/fragment/block-ref/embed/multiple/unclosed.
- `markdown/embed.rs` — no-embed passthrough, substitution, depth-limit error.
- `markdown/html.rs` — heading/code/tasklist/table rendering, full doc structure, title escaping, C67 (#420): frontmatter stripping, wikilink/pipe-order rendering, image-embed rewriting (incl. non-image-embed and multibyte-UTF-8 edge cases), known- vs unknown-callout-kind coverage.
- `canvas/mod.rs` — minimal parse, default version, invalid JSON, all node/edge types, default edge type, `extra` preservation, real Obsidian canvas, legacy `from`/`to` aliases, serialize round-trip, `file_links`.
- `config/mod.rs` — defaults-when-missing, save/load round-trip, partial-merge, env-var substitution for ai.toml (across all four config types).
- `config/env_subst.rs` — passthrough, single/multiple substitution, undefined-var error.
- `util/{slug,filename,mime,attachment}.rs` — slug edge cases, filename/path validation (forbidden chars, reserved names, length), MIME mapping/case/fallback, SHA-256 known-value/determinism, attachment-name format.
- `notion/{mod,filename,property,markdown,database,export}.rs` — zip round-trip, internal-link rewrite, callout/property-table extraction, CSV→bases, UUID strip rules, link/callout/property/bases export round-trips, missing-wikilink warning, dotfile skipping.
- `core_plugin.rs` — `import_notion`/`export_notion` dispatch through IPC, missing-source error, export round-trip, unknown-handler error; C66 (#419): `export_html` inline-HTML reply, write-to-`dest` reply, missing-source error.
- `tests/issue_78_bounds.rs` (integration) — oversize frontmatter rejection, normal-size acceptance, oversize-canvas-byte rejection, excessive-element-count rejection, realistic-canvas acceptance.
