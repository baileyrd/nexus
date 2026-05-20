# Diagnostics and observability

Operator-facing surfaces. Each plugin in this group projects a different
backend stream — LSP diagnostics, DAP sessions, kernel metrics, plugin /
session inventory, AI activity rollup, or the architecture.md drift report —
into a sidebar leaf or pane-mode view. None are required to open and edit
markdown; together they form the "what is the system doing right now" axis
of Nexus.

### debugger

- **Path:** `shell/src/plugins/nexus/debugger/`
- **Surface:**
  - View type `debugger-panel` (toolbar, call stack, scopes/variables, watch,
    breakpoints, output) hosted as a right-side leaf.
  - Command `nexus.debugger.focus`, keybinding `ctrl+shift+d` /
    `cmd+shift+d`.
  - Subscribes to seven `com.nexus.dap.*` topics (initialized, stopped,
    continued, terminated, exited, thread, output) and dispatches into the
    debugger store.
- **Depends on:** `com.nexus.dap` core plugin; adapters configured in
  `<forge>/.forge/dap.toml`.
- **Activation:** lazy — `onCommand:nexus.debugger.focus` / `onView:debugger-panel`.
- **Verdict:** Optional
- **Rationale:** A full DAP UI is a discrete feature for source-code work; a
  basic markdown forge never invokes it. Default-off in the catalog.

### diagnostics

- **Path:** `shell/src/plugins/nexus/diagnostics/`
- **Surface:**
  - Pane-mode view `nexus.diagnostics.view` with file-grouped LSP diagnostics
    and click-to-jump.
  - Activity-bar item `nexus.diagnostics.activityItem` (priority 56, alert
    icon).
  - Commands `nexus.diagnostics.show` and
    `nexus.diagnostics.openInMultibuffer` (funnels every in-forge diagnostic
    through `editor.open_excerpts`).
  - Subscribes to `com.nexus.lsp.textDocument.publishDiagnostics` globally.
- **Depends on:** `nexus.paneMode`, `nexus.activityBar`; reads from the
  editor's `EditorKernelClient`; consumes whatever `com.nexus.lsp` publishes.
- **Verdict:** Useful
- **Rationale:** Not required for plain markdown editing, but any plugin that
  emits LSP diagnostics needs somewhere for users to see them. Cheap to keep
  on; default-on in the catalog.

### healthPanel

- **Path:** `shell/src/plugins/nexus/healthPanel/`
- **Surface:**
  - View type `health-panel` — right-side leaf rendering kernel metrics:
    event-bus queue depth, IPC counts + p50/p95/p99 latency, per-capability
    granted/denied counters, per-plugin publish counters, and the
    `metrics_dropped_total` sentinel.
  - Polls `com.nexus.security::metrics_snapshot` every 5 s while the panel is
    open, plus a manual "Refresh" button.
  - Command `nexus.healthPanel.focus` (category `View`).
- **Depends on:** `com.nexus.security` (metrics_snapshot).
- **Activation:** lazy — `onCommand:nexus.healthPanel.focus` /
  `onView:health-panel`. Default-off.
- **Verdict:** Optional
- **Rationale:** Developer triage surface; explicitly default-off and aimed at
  diagnosing a chatty/slow plugin. Basic capabilities don't need it.

### observability

- **Path:** `shell/src/plugins/nexus/observability/`
- **Plugin id:** `nexus.osObservability`
- **Surface:**
  - Sidebar leaf with three tabs:
    - Usage rollup over `com.nexus.ai::activity_list`
    - Foundation workflows from `com.nexus.workflow::list`, with "Run now"
    - Vault feed filtered to `raw/`, `wiki/`, `output/`, `projects/`, `ops/`
      from `com.nexus.activity.appended`
  - Activity-bar item, commands `nexus.osObservability.show` /
    `nexus.osObservability.refresh`.
- **Depends on:** `nexus.workspace`; reads `com.nexus.ai` and
  `com.nexus.workflow`.
- **Verdict:** Optional
- **Rationale:** Targets the OS-template forge flow (`nexus forge init
  --template os`); default-off in the catalog. Not relevant to plain markdown
  capability.

### processes

- **Path:** `shell/src/plugins/nexus/processes/`
- **Surface:**
  - Pane-mode view `nexus.processes.view` listing shell plugin registry
    entries, community plugin manifests, terminal sessions
    (`com.nexus.terminal::list_sessions`), and MCP servers
    (`com.nexus.mcp.host::list_servers`).
  - Rolling kernel-event log subscribed to nine `com.nexus.<service>.` topic
    prefixes (storage, git, terminal, workflow, ai, theme, mcp, skills,
    agent).
  - Activity-bar item priority 60; command `nexus.processes.show` bound to
    `ctrl+shift+y` / `cmd+shift+y`.
  - Marked `core: true` so it can reach `api.internal.getInternalService` for
    the plugin registry.
- **Depends on:** `nexus.paneMode`, `nexus.activityBar`; consumes the internal
  `pluginList` and `communityPluginManifests` services plus terminal / MCP
  IPC.
- **Verdict:** Optional
- **Rationale:** Process manager / event firehose — useful for operators, not
  required for "open forge, edit markdown". Default-off.

### status

- **Path:** `shell/src/plugins/nexus/status/`
- **Surface:** Not a plugin. The directory contains a shared frontmatter
  `status:` cache (`statusStore.ts`, `useFileStatus.ts`) plus the
  `StatusPill` component used by the file-tree and the frontmatter metadata
  bar. There is no `index.ts` registering a manifest, and it does not appear
  in `shell/src/plugins/catalog.ts`.
- **Depends on:** consumers are `nexus/files` (tree status dots) and
  `core/editorArea/MarkdownDoc.tsx` (inline pill); reads
  `com.nexus.storage::read_frontmatter`.
- **Verdict:** Removable (as a "plugin") — but the underlying module is
  still in use.
- **Rationale:** This is a misplaced utility, not a plugin contribution. It
  should arguably move to `shell/src/lib/` or fold into `nexus/files/`.
  Distinct from `core/statusBar` (the host status-bar plugin) and
  `nexus/statusBar` (the cursor / word-count contributions); the three names
  collide and are worth flagging.

### osArchitecture

- **Path:** `shell/src/plugins/nexus/osArchitecture/`
- **Surface:**
  - Sidebar leaf rendering `architecture.md` parsed into a domain → task
    hierarchy, with drift detection against the live skill / workflow
    registries.
  - Commands `nexus.osArchitecture.show` / `nexus.osArchitecture.refresh`.
  - Reads `com.nexus.storage::read_file`, `com.nexus.skills`, and
    `com.nexus.workflow`.
- **Depends on:** `nexus.workspace`; skills + workflow surfaces are soft deps
  (drift detection is best-effort).
- **Verdict:** Optional
- **Rationale:** A diagnostic surface specific to the OS-template forge.
  Default-off; not relevant to a plain markdown forge.

## Category verdict

| Plugin         | Verdict   | Required for basic capabilities |
|----------------|-----------|---------------------------------|
| debugger       | Optional  | No — DAP UI, out of scope for markdown editing |
| diagnostics    | Useful    | No — but every LSP-emitting plugin needs a sink |
| healthPanel    | Optional  | No — developer triage, default-off |
| observability  | Optional  | No — OS-template feature, default-off |
| processes      | Optional  | No — operator surface, default-off |
| status         | Removable | N/A — not a plugin; shared utility module misfiled under `plugins/nexus/` |
| osArchitecture | Optional  | No — OS-template feature, default-off |
