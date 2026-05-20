# Workspace Chrome

This category covers the shell's outer skeleton: the workspace lifecycle that
boots the kernel against a forge, the side-dock glue plugins that wrap the
shell-core slot infrastructure, the right-side status bar contributors, the
activity timeline pane, and the theme picker. Several entries pair with a
shell-core plugin of the same name — see the core-vs-nexus note on each.

### nexus.workspace

- **Path:** `shell/src/plugins/nexus/workspace/`
- **Surface:** Contributes commands `nexus.workspace.open`, `setRoot`,
  `openWithTemplate`, `openRemote`, `close`; binds `Ctrl+O` / `Cmd+O`; sets
  context keys `nexus.workspace.rootPath` and `nexus.workspace.hasRoot`;
  emits `workspace:opened` / `workspace:closed`; registers
  `WorkspaceStatusItem` into the `statusBarLeft` slot. Drives the kernel
  lifecycle via the Tauri commands `init_forge`, `boot_kernel`,
  `shutdown_kernel`, `boot_remote`, `kernel_is_booted`, `path_exists`,
  `get_shell_state`. Persistence: `rootPath` in plugin-local localStorage,
  with fallback to `shell-state.lastForgePath`. Layout state lives in
  `<forge>/.forge/workspace.json` (managed by the `workspace` singleton in
  `shell/src/workspace`).
- **Depends on:** Tauri host commands; `@tauri-apps/plugin-dialog` for the
  folder picker; downstream every other nexus plugin keys off
  `workspace:opened` / `closed`.
- **Verdict:** Essential
- **Rationale:** The kernel does not boot until this plugin calls
  `init_forge` + `boot_kernel`. Without it the shell cannot open a forge,
  no IPC handler is reachable, and every dependent plugin sees a permanently
  unavailable kernel. Load-bearing for the basic workflow.

### nexus.sidebar

- **Path:** `shell/src/plugins/nexus/sidebar/`
- **Surface:** Manifest-only stub. The `activate` hook is an explicit no-op
  retained so existing `dependsOn: ['nexus.sidebar']` declarations across
  the tree still resolve without a workspace-wide rename.
- **Core-vs-nexus:** Both `core.sidebar` (`shell/src/plugins/core/sidebar/`)
  and `nexus.sidebar` are Phase 7 legacy stubs — neither plugin renders
  anything today. The left dock is drawn directly by `<Workspace>` once
  workspace state is hydrated; feature plugins call
  `workspace.ensureLeafOfType(...)` + `workspace.revealLeaf(...)` rather
  than registering into a `sidebar` slot. `core.sidebar` is not even loaded
  from `main.tsx`. `nexus.sidebar` is loaded only as a dependency anchor.
- **Depends on:** Nothing. Dependents: many feature plugins still list it in
  `dependsOn` (templates, workflow, mcp, files, etc.).
- **Verdict:** Useful
- **Rationale:** Not load-bearing — its activate is empty — but flipping it
  to absent would break dependency resolution for ~10 nexus plugins until
  every `dependsOn` is scrubbed. Keep the stub until that cleanup ships.

### nexus.rightPanel

- **Path:** `shell/src/plugins/nexus/rightPanel/`
- **Surface:** Registers command `nexus.rightPanel.toggle` bound to
  `Ctrl+Alt+R` / `Cmd+Alt+R`; mirrors the right side-dock collapsed state
  into context key `nexus.rightPanel.visible`; maintains a tab-metadata
  store via the `rightPanel:registerTab` / `unregisterTab` bus events;
  drives the side-dock through `workspace.setSidedockCollapsed('right',
  ...)`. No views registered into a slot — Phase 7 removed the
  `slot:'rightPanel'` host.
- **Core-vs-nexus:** `core.rightPanel` (`shell/src/plugins/core/rightPanel/`)
  is also a Phase 7 stub — its toggle command is duplicated and points at
  the same `workspace.setSidedockCollapsed` API, and the file's header
  comment explicitly states it is *not* loaded from `main.tsx`. The active
  plugin is `nexus.rightPanel`; `core.rightPanel` is dead code retained as
  a template.
- **Depends on:** the workspace layout singleton; the bus events come from
  feature plugins that contribute right-rail tabs (file properties,
  bookmarks, etc.).
- **Verdict:** Useful
- **Rationale:** Provides the only keybinding + context key for toggling
  the right rail and the legacy tab-registration bus that several
  inspectors still emit on. Not strictly required for browse / edit /
  search / git, but a typical user would notice the missing shortcut and
  the rail's metadata going dim.

### nexus.statusBar

- **Path:** `shell/src/plugins/nexus/statusBar/`
- **Surface:** Registers three `statusBarRight` views — `WorkspaceStatus`
  (forge sync dot), `FileStats` (word/char/backlink counts for the active
  note, reads `useBacklinksStore`), and `IndexingStatus` (polls
  `com.nexus.ai::index_status` every 2 s, exposes a "Reindex" click that
  invokes `com.nexus.ai::index_trigger`). `dependsOn: ['nexus.workspace',
  'nexus.editor']`.
- **Core-vs-nexus:** `core.statusBar` (`shell/src/plugins/core/statusBar/`)
  is the infrastructure plugin — it registers the `statusBarLeft` /
  `statusBarRight` render surfaces themselves and seeds them with the
  default placeholder items (`statusBar.sync`, `statusBar.branch`,
  `statusBar.index`, `statusBar.plugins`, `statusBar.position`,
  `statusBar.encoding`, `statusBar.count`, `statusBar.backlinks`).
  `nexus.statusBar` is the *content* contributor — its three views replace
  the placeholders with live data. The two cooperate: core.statusBar owns
  the slot, nexus.statusBar owns the items.
- **Depends on:** core.statusBar render slots; `com.nexus.ai` for indexing
  status; `nexus.editor` + `nexus.backlinks` stores for FileStats.
- **Verdict:** Useful
- **Rationale:** Adds the live status indicators (indexing progress,
  per-file counts) that the placeholder items in `core.statusBar` only
  fake. Removing it leaves the status bar visually static but does not
  break browse / edit / search / git.

### nexus.activity (activityTimeline)

- **Path:** `shell/src/plugins/nexus/activityTimeline/`
- **Surface:** Plugin id is `nexus.activity` (renamed from
  `nexus.activityTimeline` with `legacyPluginIds` migration). Registers
  `nexus.activityTimeline.view` into the `paneMode` slot, an activity-bar
  item, and commands `nexus.activityTimeline.show` /
  `nexus.activityTimeline.clear`. Hydrates from
  `com.nexus.ai::activity_list`, subscribes to the universal
  `com.nexus.activity.appended` topic and the legacy
  `com.nexus.ai.activity_appended` topic, dedupes by id.
  `dependsOn: ['nexus.paneMode', 'nexus.activityBar']`.
- **Depends on:** `com.nexus.ai` for the persisted JSONL log; bus events
  from any emitter (AI calls, file writes, git commits, terminal sessions,
  workflow runs); `nexus.paneMode` host.
- **Verdict:** Optional
- **Rationale:** Diagnostic / audit pane. Useful to power users tracing
  what the agent / workflows touched, but absent from the basic browse /
  edit / search / git path. The on-disk JSONL is owned by the AI plugin,
  not this one, so removing the pane loses only the viewer.

### nexus.themePicker

- **Path:** `shell/src/plugins/nexus/themePicker/`
- **Surface:** Registers commands `nexus.themePicker.open` /
  `close` (bound to `Ctrl+Shift+T` / `Cmd+Shift+T`, with `escape` to
  dismiss while open); context key `nexus.themePicker.visible`; renders
  `ThemePicker` into the `overlay` slot with a `ThemeBuilder` sub-view;
  activity-bar entry "Themes" with the `sliders` icon; clears its swatch
  cache on `THEME_CHANGED_EVENT`. `dependsOn: ['nexus.activityBar']`.
- **Core-vs-nexus:** `core.theme-service` (`shell/src/plugins/core/themeService/`)
  is the kernel bridge — it subscribes to `com.nexus.theme.changed` and
  re-hydrates `useThemeStore` so the cascade of CSS variables on
  `<body>` stays in sync with the backend `crates/nexus-theme` plugin.
  `nexus.themePicker` is the *UI* — a picker overlay + builder that
  invokes IPC against `com.nexus.theme` to switch theme / mode / enabled
  snippets. core handles state, nexus handles selection.
- **Depends on:** `core.theme-service` + `useThemeStore`; backend
  `com.nexus.theme` for the theme catalog; `nexus.activityBar` host.
- **Verdict:** Useful
- **Rationale:** Only UI for changing theme without hand-editing
  `<forge>/.forge/theme.toml`. Not strictly required for browse / edit
  but absence would force a manual config edit + reload to switch
  appearance — a typical user would notice immediately.

## Category verdict

| Plugin                  | Verdict   | Required for basic workflow |
|-------------------------|-----------|-----------------------------|
| `nexus.workspace`       | Essential | Yes — boots kernel and owns forge lifecycle |
| `nexus.sidebar`         | Useful    | No — dep-graph anchor only, activate is no-op |
| `nexus.rightPanel`      | Useful    | No — toggle keybinding + tab metadata for right rail |
| `nexus.statusBar`       | Useful    | No — live indexing / file-stats indicators |
| `nexus.activity`        | Optional  | No — audit/timeline viewer |
| `nexus.themePicker`     | Useful    | No — only UI for theme switching |

The core-vs-nexus pattern in this category is uneven:
- `core.sidebar` and `core.rightPanel` are Phase 7 stubs — not loaded;
  the active code is the `nexus.*` half.
- `core.statusBar` and `core.theme-service` are live and cooperate with
  their `nexus.*` siblings — core owns the substrate (slots,
  subscriptions), nexus owns the content (status items, picker UI).
