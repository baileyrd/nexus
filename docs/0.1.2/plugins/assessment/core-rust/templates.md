# com.nexus.templates

- **Path:** `crates/nexus-templates/`
- **Tier:** Core Rust
- **Bootstrap order:** 12

## Architecture

- Entry point: `crates/nexus-templates/src/core_plugin.rs` — `TemplatesCorePlugin::open(forge_root)`. Bootstrap registration: `crates/nexus-bootstrap/src/plugins/templates.rs`. Lifecycle: `LifecycleFlags::NONE`.
- Key modules:
  - `template.rs` — `parse_template_file` / `parse_template_text`: YAML frontmatter (name, description, `target_path`, `parameters`) + markdown body.
  - `registry.rs` — `TemplateRegistry`: walks `<forge>/.forge/templates/` (recurses sub-dirs), plus a built-in set merged in via `builtins.rs` so the plugin works on a fresh forge with no setup.
  - `substitute.rs` — `render` substitutes `{{param}}` and built-in variables (`today`, `now`, `forge_path`) into both the body and the `target_path`.
  - `builtins.rs` — embedded built-in templates (meeting-notes, etc.).
- Persistence:
  - `<forge>/.forge/templates/<sub>/<name>.template.md` — user-authored template files (file-as-truth). Built-ins live in the binary; user files override by id.
  - `apply` writes the rendered body to the resolved `target_path` under the forge root — that file is a regular markdown note, not template-private state.
- Settings owned: none. No TOML config file.
- External dependencies: `serde_yml`, `chrono` (for `today` / `now` built-in variables). Read-mostly — no network, no SQLite, no subprocess.

## Surface

5 IPC handlers (full table at `crates/nexus-templates/src/core_plugin.rs:117`):

`list`, `get`, `render`, `apply`, `reload`.

## Necessity

- **Verdict:** Useful
- **Required for basic capabilities?** No — users can hand-craft any markdown file they need. Templates accelerate note creation but the basic-capability workflow (open / browse / edit / search / commit) needs none of them.
- **Depended on by:** shell-nexus `templates` panel (extension-system category), command palette's "New from template" affordance. No backend plugin imports `nexus-templates` — every consumer routes through IPC.
- **Depends on:** `nexus-plugins` only. No kernel context required for `list` / `get` / `render`; `apply` writes through standard file I/O against the forge root.
- **What breaks if removed:** the templates panel and the "New from template" command palette entry disappear; users still create and edit any markdown they like by hand. Bootstrap registration drops cleanly because nothing else hard-depends on it.

## Notes

- Sits one tier above Optional because note-creation aid is a workflow nicety most markdown-tool users come to expect; rated Useful for that reason. If the product wants to ship a hyper-minimal core, this plugin can be removed without functional loss — drop it down to Optional in that scenario.
- Built-ins are merged into the user registry on every `reload` so an empty `.forge/templates/` directory still yields a usable list.
- `target_path` substitution is the only path-shaping mechanism — there's no template-private slug generation; whatever the user writes in the frontmatter pattern is what `apply` resolves to.
