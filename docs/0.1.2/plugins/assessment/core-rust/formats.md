# com.nexus.formats

- **Path:** `crates/nexus-formats/`
- **Tier:** Core Rust
- **Bootstrap order:** 10 (after editor/theme/ai-runtime/ai/skills/templates)

## Architecture

- Entry point: `crates/nexus-formats/src/core_plugin.rs` (`FormatsCorePlugin::open(forge_root)`).
- Bootstrap wiring: `crates/nexus-bootstrap/src/plugins/formats.rs:17` — `LifecycleFlags::NONE`, manifest from `IPC_HANDLERS`, no `plugin.toml` on disk.
- Key modules:
  - `markdown/` — markdown parser (comrak-based) plus block / link / property / task extraction. **Library** consumed by `nexus-storage::parser` and `nexus-editor`; the markdown parser is **not** exposed via the IPC surface.
  - `canvas/` — canvas file parsing/serialization.
  - `config/` — `AppConfig`, `WorkspaceState`, `AiConfig`, `McpConfig`, env-var substitution. Re-exported by `nexus-storage::config` (which adds the I/O).
  - `notion/` — Notion zip-import and Notion-format export converters. The only IPC-exposed surface.
  - `migration.rs`, `util/`.
- Persistence: none. The plugin holds only the forge root; conversions read source paths and write to caller-supplied destinations.
- Settings owned: the **types** for `app.toml` / `workspace.json` / `ai.toml` / `mcp.toml` live here (see `docs/0.1.2/settings/forge-config.md` — `AppConfig` at `nexus-formats/src/config/app.rs:9`), but the I/O is owned by `nexus-storage::config`.
- External dependencies of note: `comrak`, `serde_yml`, `toml`, `zip`, `sha2`, `chrono`, `csv`. No native libs, no network.

## Surface

IPC commands (from `core_plugin.rs:84` `IPC_HANDLERS`):

| Id | Command         | Args                                  | Purpose                          |
|---:|-----------------|---------------------------------------|----------------------------------|
|  1 | `import_notion` | `{ source: PathBuf, dest?: PathBuf }` | Import a Notion zip-export       |
|  2 | `export_notion` | `{ source?: PathBuf, dest: PathBuf }` | Export forge subdir to Notion    |

No events, no lifecycle hooks.

## Necessity

- **Verdict:** Essential (as a library); Optional (as a core plugin).
- **Required for basic capabilities?** Yes — but indirectly. The **crate** is load-bearing: `nexus-storage` depends on `nexus-formats` for markdown parsing, canvas, and config schemas, and `nexus-editor` depends on it for serialization. Without the crate, neither storage nor editor compiles. The **IPC plugin surface**, by contrast, only exposes Notion import/export — those handlers are not on the basic-capabilities path.
- **Depended on by:** `nexus-storage`, `nexus-editor` (Cargo dependencies). No other plugins call its IPC commands as part of normal browse/edit/search flows.
- **Depends on:** `nexus-plugins` only (no kernel dep — the plugin holds no event bus reference).
- **What breaks if removed (crate):** storage and editor stop compiling — markdown parsing, canvas, config schemas all live here. If only the IPC plugin registration is removed, Notion import/export is unreachable; everything else keeps working.

## Notes

- Bootstrap classifies this as `LifecycleFlags::NONE`: no `on_init` / `on_start` / `on_stop` work. Both handlers are synchronous I/O wrappers — fine because the kernel dispatches each call on a dedicated thread.
- The `[features] ts-export` flag emits TypeScript + JSON Schema bindings for the IPC arg DTOs. Off by default.
- The crate's outsized importance comes from the library half, not the IPC plugin half. A future split could leave Notion converters in the plugin and move the rest to a more clearly named `nexus-parsers` crate; today the names are conflated.
