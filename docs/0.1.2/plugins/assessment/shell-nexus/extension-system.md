# Extension System

This category covers shell-side frontends for backend extensibility
subsystems: the plugins management surface in Settings, the workflow
runner, the page-templates engine, and the Bases (database-style table
views over markdown) UI. Each is a thin React + zustand layer over IPC
into a `com.nexus.*` core plugin — the heavy lifting is in Rust.

### nexus.pluginsMgmt

- **Path:** `shell/src/plugins/nexus/pluginsMgmt/`
- **Surface:** Registers `nexus.pluginsMgmt.overlay` into the `overlay`
  slot (recently flipped to inline rendering in Settings rather than a
  modal — commits `bf1a1341`, `9c53cbca`, `dee79302`). Commands
  `nexus.plugins.open` (`Ctrl+Shift+X`), `close`, `toggleCommunity`,
  `reviewCapabilities`, `enableBuiltin`, `disableBuiltin`, `configure`;
  context key `nexus.plugins.visible`. Marked `core: true` in the
  manifest so it can reach `api.internal.getInternalService` for the
  `pluginList`, `communityPluginManifests`, `communityPluginDenied`,
  and `availablePlugins` services. Calls Tauri commands
  `get_plugin_granted_capabilities`, `set_plugin_enabled`, plus the
  `enableBuiltinPlugin` / `disableBuiltinPlugin` helpers in
  `host/pluginActivation`.
- **Depends on:** shell extension host internals (`shellRegistry`,
  `pluginActivation`, `pluginsStatusStore`); `capabilityPrompt` core
  plugin for the consent-review modal; backend Tauri command surface
  for community plugin enable/disable + grant management.
- **Verdict:** Useful
- **Rationale:** Without this surface a user cannot enable a
  default-off built-in, install or toggle a community plugin, or
  review previously granted capabilities — they would have to hand-edit
  `<forge>/.forge/plugins.json` (or `granted_caps.json`) and reload.
  Not on the basic browse/edit/search/git path, but a typical Nexus
  install relies on plugins, and this is the only ergonomic management
  surface.

### nexus.workflow

- **Path:** `shell/src/plugins/nexus/workflow/`
- **Surface:** Registers the `workflow` view type via
  `api.viewRegistry.register('workflow', ...)`, an activity-bar entry
  with the `bolt` icon, and commands `nexus.workflow.refresh`,
  `show`, `validate`. Lists, runs, and validates workflows by IPC to
  `com.nexus.workflow` (`list`, `run`, `validate`). Run timeout is
  bumped to `LONG_RUNNING_OP_TIMEOUT_MS` because steps can spawn
  agent runs / terminal commands / AI calls. Refreshes on
  `workspace:opened`, clears state on `workspace:closed`.
  `dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar']`.
- **Depends on:** `com.nexus.workflow` core plugin (the workflow
  engine and `.workflow.toml` loader); `nexus.activityBar` host;
  workspace lifecycle events.
- **Verdict:** Optional
- **Rationale:** Frontend for a power-user automation feature. The
  basic browse / edit / search / git workflow doesn't involve TOML
  workflow definitions. Removing the pane would not affect note
  editing — only users who author `.workflow.toml` files would notice.

### nexus.templates

- **Path:** `shell/src/plugins/nexus/templates/`
- **Surface:** Registers the `templates` view type, activity-bar
  entry (`template` icon, priority 45), and commands
  `nexus.templates.new`, `list`, `show`, `refresh`. Calls
  `com.nexus.templates::list` and `::apply` to enumerate and
  instantiate templates. The "New" flow prompts for each declared
  parameter in order and emits a `nexus.files.openByPath` to reveal
  the freshly-created note.
  `dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar']`.
- **Depends on:** `com.nexus.templates` core plugin (template engine
  in `crates/nexus-templates`); `nexus.files` for the open-after-apply
  step; activity bar host.
- **Verdict:** Useful
- **Rationale:** Not strictly required for the basic workflow — a
  user can create notes by typing `Ctrl+N` (or whatever the files
  plugin binds) — but markdown forges are heavily template-driven in
  practice (daily notes, project journals, meeting templates). A
  typical Nexus user would miss it within hours.

### nexus.bases

- **Path:** `shell/src/plugins/nexus/bases/`
- **Surface:** Claims the `.bases` (directory, multi-file YAML) and
  `.base` (Obsidian single-file YAML, read-only — ADR 0019) file
  extensions via `api.viewRegistry.registerExtensions(...)`. Registers
  the `bases` view type (a fully editable table / board / calendar /
  gallery / list / timeline surface with schema editor, clipboard
  integration, undo/redo). Commands `nexus.bases.new`, `undo`, `redo`,
  `cut`, `copy`, `paste` — the edit chords are gated on
  `bases.focused && !bases.editing` so they don't steal keystrokes
  from cell editors. Routes IPC through `makeBasesKernelClient` to
  `com.nexus.storage` (handlers `base_read`, `base_index`, `base_query`,
  `base_list` plus the 40-48 CRUD range). Setting
  `nexus.bases.fileExtensions` overrides the default extensions.
  `dependsOn: ['nexus.workspace']`.
- **Depends on:** `com.nexus.storage` (base_* IPC family); workspace
  lifecycle; configuration service for the extensions setting.
- **Verdict:** Optional
- **Rationale:** Database-style table views over markdown frontmatter.
  Genuinely powerful for property-heavy forges, but firmly a
  feature opt-in — the basic browse / edit / search / git path does not
  touch `.bases`. Removing it makes `.bases` files fall through to
  CodeMirror as raw YAML, which is a usable fallback.

## Category verdict

| Plugin              | Verdict   | Required for basic workflow |
|---------------------|-----------|-----------------------------|
| `nexus.pluginsMgmt` | Useful    | No — only UI for managing built-ins / community plugins |
| `nexus.workflow`    | Optional  | No — frontend for backend workflow engine |
| `nexus.templates`   | Useful    | No — note creation aid, not on the basic path but high-value |
| `nexus.bases`       | Optional  | No — database table views over markdown |

All four plugins follow the same architectural shape: a Rust core
plugin owns the data and engine, the shell plugin is a React +
zustand frontend that talks to it over `api.kernel.invoke`. None
register direct Tauri handlers — all extensibility flows through IPC.
