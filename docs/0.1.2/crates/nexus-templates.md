# nexus-templates

> Kind: lib · IPC plugin id: com.nexus.templates · CorePlugin: yes · Has settings: no · As of: 2026-05-25

## Overview
`nexus-templates` is the page-template subsystem. A **template** is a `.template.md` file: YAML frontmatter (name, optional description, optional `target_path` pattern, declared `parameters`) followed by a markdown body. Applying a template substitutes `{{var}}` placeholders in both the body and the target path, then writes the rendered body to a forge-relative destination. The intent is scaffolding — meeting notes, daily journals, Notion-style pages — not general-purpose templating; the substitution engine is deliberately minimal (no conditionals, loops, or filters; community plugins are the escape hatch for richer needs).

Templates come from two layered sources merged in a [`TemplateRegistry`]: a small **built-in seed set** compiled into the crate (`notion-page`, `notion-database-row`, `daily-journal`, `meeting-notes`) and **user templates** discovered by recursively walking `<forge>/.forge/templates/` for files ending in `.template.md`. A user template whose `name` frontmatter matches a built-in **overrides** it. This honours file-as-truth: the on-disk `.template.md` files are authoritative for user content, the built-ins exist only to give a fresh forge a useful default set without writing anything to disk.

At apply time the engine seeds three built-in variables (`today`, `now`, `forge_path`) from `chrono::Utc::now()` and the forge root, layers user-supplied args on top (so a caller can override `today`), then fills any still-missing declared parameters from their `default` (itself substituted, so a default may reference `{{today}}`) or errors if the parameter is `required`. Rendering produces the body plus a resolved target path; `apply` validates the path stays inside the destination root and writes the file.

Microkernel fit: the crate is a leaf service depending only on `nexus-plugins` for the `CorePlugin` trait. It owns no kernel state and reaches nothing else — all access flows through five IPC handlers registered by `nexus-bootstrap`, so the shell command palette, the CLI, the MCP server, and community plugins consume templates uniformly via `ipc_call` rather than linking the crate.

## Position in the dependency graph
- **Direct nexus-\* deps:** `nexus-plugins` (the `CorePlugin` / `PluginError` contract and `define_dispatch_helpers!` macro).
- **Notable external deps:** `chrono` (built-in date/time variables), `serde` + `serde_json` + `serde_yml` (frontmatter parse, IPC (de)serialization), `thiserror` (error enums), `tracing` (degraded-load warnings). `ts-rs` + `schemars` are optional, gated behind the `ts-export` feature, and used only to emit TypeScript/JSON-Schema bindings for the IPC arg structs. `tempfile` is a dev-dependency.
- **Crates depending on it:** `nexus-bootstrap` (registers the core plugin) and `nexus-cli` (template subcommands). No other crate links it directly — everyone else routes through IPC.

## Public API surface
**`lib.rs`** — re-exports the crate's surface from the five modules below.

**`template.rs`** — parsing, rendering, application.
- `Template` — a parsed `.template.md`: `meta: TemplateMeta` (flattened) + `body: String`.
- `TemplateMeta` — frontmatter projection: `name`, optional `description`, optional `target_path`, `parameters: Vec<TemplateParameter>`, plus `extra: BTreeMap<String, serde_yml::Value>` capturing unknown fields so the schema can grow.
- `TemplateParameter` — one declared param: `name`, `r#type: ParameterType` (default `String`), optional `default`, `required: bool`, optional `description`.
- `ParameterType` — UI hint enum `{String, Number, Boolean, Date}` (lowercase serde); runtime stores everything as strings.
- `parse_template_file(path)` / `parse_template_text(input, file)` — split frontmatter from body and deserialize the YAML into `TemplateMeta`.
- `Template::resolve_values(user_args, forge_root)` — compute the final value map (built-ins + user args + applied defaults); errors on missing required params.
- `Template::render(values)` — substitute body and `target_path` (target defaults to `{{name}}.md`), collapsing a `.md.md` double extension.
- `Template::apply(user_args, dest_root, overwrite)` — resolve + render + write; returns the absolute path written.
- `TemplateParseError`, `ApplyError` — error enums.

**`substitute.rs`** — the substitution engine.
- `render(input, values)` — replace `{{var}}` placeholders; returns rendered string.
- `SubstitutionError` — `UnknownVariable { name }`, `MalformedTag { line }`.

**`registry.rs`** — discovery / index.
- `TemplateRegistry` — in-memory `HashMap<String, Template>` keyed by name.
- `TemplateRegistry::load(forge_root)` — built-ins then user templates layered on top.
- `empty()`, `insert(tpl)`, `get(name)`, `iter()`, `len()`, `is_empty()`, `list()` (sorted `(name, description)` pairs).
- `TemplateRegistryError` — `Io`, `Parse`.

**`builtins.rs`** — compiled-in seed templates.
- `all()` — `Vec<(filename, body)>` of the four built-ins.
- `parsed()` — parse every built-in into `Template` (panics on a corrupt built-in; tests guard this).

**`core_plugin.rs`** — the IPC core plugin.
- `TemplatesCorePlugin` (holds `forge_root` + `Mutex<TemplateRegistry>`), `open(forge_root)`.
- IPC arg structs: `GetPageTemplateArgs`, `RenderTemplateArgs`, `ApplyTemplateArgs`.
- Constants: `PLUGIN_ID`, `HANDLER_LIST/GET/RENDER/APPLY/RELOAD`, `IPC_HANDLERS`.

## IPC handlers
Plugin id `com.nexus.templates`. Handler ids are append-only; `nexus-bootstrap` mirrors `.v1` aliases onto each command (ADR 0021).

| Command (id) | Args | Returns | Capability | Description |
|---|---|---|---|---|
| `list` (1) | `{}` | JSON array of `{ name, description, target_path, parameters }`, sorted by name | unrestricted | Every template in the registry. |
| `get` (2) | `GetPageTemplateArgs { name }` | The full `Template` (frontmatter flattened + `body`); error if not found | unrestricted | One template by name. |
| `render` (3) | `RenderTemplateArgs { name, args?: {string→string} }` | `{ name, target_path, body }` | unrestricted | Dry-run render — resolves values and substitutes, does **not** write. |
| `apply` (4) | `ApplyTemplateArgs { name, args?: {…}, target?: string, overwrite?: bool }` | `{ name, path (forge-relative), absolute_path }` | unrestricted (downstream `fs.write`) | Render and write to disk; `target` overrides the template's `target_path`. |
| `reload` (5) | `{}` | `{ loaded: <count> }` | unrestricted | Re-scan `<forge>/.forge/templates/` and rebuild the registry. |

Per `docs/0.1.2/ipc-handlers.md`, all five are classified `unrestricted` (`apply` does a downstream `fs.write`). Arg structs use `#[serde(deny_unknown_fields)]`; `args` and the `apply` optionals default via `#[serde(default)]`.

## Capabilities
None declared or checked inside this crate. The crate performs no capability checks itself; all five handlers are classified `unrestricted` in the bootstrap capability matrix. Path-safety (rejecting absolute or `..`-containing target paths) is enforced directly in `Template::apply`, not via a capability.

## Settings / Config
No config struct and no `.forge/*.toml` file — the crate is settings-free. Its "configuration" is the templates themselves on disk.

**Template file format** (`<name>.template.md`):
```text
---
name: meeting-notes              # required, unique key
description: …                   # optional, shown in pickers
target_path: meetings/{{today}} - {{title}}.md   # optional; default {{name}}.md
parameters:                      # optional
  - name: title
    type: string                 # string | number | boolean | date (UI hint; default string)
    required: true               # default false
    default: "{{today}}"         # optional; substituted at apply time
    description: …               # optional
---
# {{title}}
…body…
```

**Placeholder syntax** (`substitute.rs`): `{{var}}`; whitespace inside braces is allowed (`{{ var }}`). Escape: `{{!}}` emits a literal `{{`. Unknown variables → `UnknownVariable`. An unbalanced `{{` (no closing `}}` before end-of-line) or a nested `{{` inside a tag → `MalformedTag { line }` (1-based line). No conditionals, loops, or filters.

**Built-in variables** (seeded on every apply/render):
- `today` — `YYYY-MM-DD` (UTC, `chrono::Utc::now()`).
- `now` — RFC-3339 timestamp (UTC).
- `forge_path` — absolute path of the forge root.

User-supplied args override built-ins (e.g. a caller may pass `today=1999-01-01`).

**Discovery defaults:** user templates live in `<forge>/.forge/templates/` (recursive; sub-directories supported). Only files ending in `.template.md` are loaded; others are ignored. Same-name user templates override built-ins.

## Events
None. The crate neither publishes nor subscribes to any event-bus topic.

## Internals & notable implementation details
- **Frontmatter parsing** (`parse_template_text`): requires the file to start with `---\n`, finds the closing `\n---\n` separator (tolerating a trailing `\n---` at EOF with no body), feeds the slice between them to `serde_yml`, and treats the remainder as the body. Missing or malformed delimiters yield `MissingFrontmatter`; YAML that parses but doesn't match `TemplateMeta` yields `SchemaError`.
- **Substitution algorithm** (`render`): a single linear byte scan. On `{{` it first checks the 5-byte escape `{{!}}`, otherwise scans for the closing `}}` on the same line via `find_close` (a `\n` before `}}` aborts → `MalformedTag`). The inner text is trimmed; empty or containing a stray `{` → `MalformedTag`. Otherwise the trimmed name is looked up in the value map (`UnknownVariable` if absent). Line counting increments on `\n` for accurate error positions. Output is built into a pre-sized `String`. (Note: the byte-by-byte non-`{{` path pushes `bytes[i] as char`, so it assumes ASCII-or-byte content for the literal segments.)
- **Value resolution order** (`resolve_values`): built-ins → user args (override) → per-parameter defaults for anything still missing. A default is itself rendered against the accumulated values, so `default: "{{today}}"` works. A missing param that is `required` errors with `MissingParameter`; a missing optional param defaults to the empty string.
- **Target-path handling** (`render` + `apply`): target defaults to `{{name}}.md`; a rendered `…​.md.md` (e.g. when a `title` arg already ends in `.md`) is collapsed to a single `.md`. `apply` rejects absolute paths or any path containing `..` (`PathEscape`), creates parent directories, and refuses to overwrite an existing file unless `overwrite=true` (`AlreadyExists`).
- **Registry construction** (`load`): inserts built-ins first, then recursively walks `<forge>/.forge/templates/` via `visit_templates`, inserting/overriding by `name`. A single malformed user file aborts the whole load with `Parse`. The order returned by `iter()` is unstable (HashMap); `list()` and the `list` IPC handler both sort by name for deterministic output.
- **Core plugin** (`TemplatesCorePlugin`): wraps the registry in a `Mutex` to stay `Send + Sync` for kernel dispatch. `open` loads eagerly and degrades to an empty registry with a `tracing::warn!` on load failure (so a broken templates dir doesn't break boot); `reload` does the same. `apply` clones the template to patch `target_path` when the caller supplies `target`, and returns both the forge-relative `path` and the `absolute_path`. A poisoned mutex surfaces as an exec error. `IPC_HANDLERS` is the single source of truth for `(command, id)` pairs (SD-06), consumed by `nexus_bootstrap::plugins::templates::register`.

## Tests
Unit tests are colocated (`#[cfg(test)]` modules); there is **no `tests/` directory in this crate** — the end-to-end IPC tests live in `nexus-bootstrap`.

- `template.rs` (13 tests): minimal/parameterized parsing, missing frontmatter, missing closing separator, missing required param, default-with-substitution, user-arg override of built-ins, target-path pattern rendering, `.md.md` collapse, overwrite refusal/allowance, path-escape rejection, unknown-param default fallback.
- `substitute.rs` (8 tests): simple substitution, whitespace tolerance, unknown-variable error, unclosed-tag malformed, nested-open-brace malformed, escape emits literal braces, line-count advancement, multiple substitutions.
- `registry.rs` (6 tests): empty forge returns only built-ins, user override of built-in, sub-dir loading, malformed user template surfaces error, sorted `list()`, ignores non-`.template.md` files.
- `builtins.rs` (3 tests): every built-in parses, built-in names unique, built-in filenames unique.
- `core_plugin.rs` (9 tests): list returns built-ins, get known/unknown, render dry-run, apply writes file, apply with target override, apply refuses overwrite, reload picks up new user template.
- `nexus-bootstrap/tests/templates_ipc.rs` (5 tests): list, apply-writes-file, render-is-dry-run, unknown-template error, user-template-overrides-builtin — all driven through the real kernel `ipc_call` surface on a scratch forge.
