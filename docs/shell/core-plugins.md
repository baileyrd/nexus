# Core Plugins

Core plugins are shipped with the shell binary. They differ from community plugins in three ways: they load first, they receive `api.internal.*`, and they define the infrastructure that community plugins build on.

Core plugins split into two categories: **service plugins** (infrastructure, no UI) and **UI plugins** (consume services, render into slots).

---

## Load Order

```
Phase 1 — Service plugins (no dependencies on each other)
  core.configuration-service
  core.notification-service
  core.filesystem-service
  core.theme-service
  core.language-service

Phase 2 — UI plugins (depend on services)
  core.title-bar
  core.activity-bar
  core.sidebar
  core.editor-area
  core.panel-area
  core.status-bar
  core.command-palette
  core.settings               ← depends on core.configuration-service
  core.notifications-ui       ← depends on core.notification-service
  core.theme-picker           ← depends on core.theme-service, core.configuration-service

Phase 3 — Feature plugins (depend on services + UI)
  core.file-explorer          ← depends on core.filesystem-service, core.sidebar
  core.terminal               ← depends on core.panel-area
  core.search                 ← depends on core.filesystem-service, core.sidebar
```

---

## Service Plugins

### core.configuration-service

Bootstraps the configuration registry and config store. After this loads, `api.configuration` is available to all subsequent plugins.

```typescript
export const configurationServicePlugin: Plugin = {
  manifest: {
    id: 'core.configuration-service',
    name: 'Configuration Service',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {}
  },

  activate(api: PluginAPI) {
    const configRegistry = new ConfigurationRegistry()
    const configStore = new ConfigStore()

    api.internal.registerInternalService('configurationRegistry', configRegistry)
    api.internal.registerInternalService('configStore', configStore)
    // Now api.configuration.register() and api.configuration.getValue() work
  }
}
```

**Provides:** `api.configuration.register()`, `api.configuration.getValue()`, `api.configuration.setValue()`

---

### core.notification-service

Manages a notification queue. Plugins push notifications; the UI plugin (`core.notifications-ui`) renders them.

```typescript
export const notificationServicePlugin: Plugin = {
  manifest: {
    id: 'core.notification-service',
    name: 'Notification Service',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {}
  },

  activate(api: PluginAPI) {
    api.internal.registerInternalService(
      'notificationQueue',
      new NotificationQueue()
    )
    // Now api.notifications.show() works
  }
}
```

**Provides:** `api.notifications.show({ message, type, duration, actions })`

---

### core.filesystem-service

Wraps Tauri's filesystem API into a sanctioned abstraction. Enforces path scoping — community plugins can only access paths within their declared scope.

```typescript
export const filesystemServicePlugin: Plugin = {
  manifest: {
    id: 'core.filesystem-service',
    name: 'Filesystem Service',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {}
  },

  activate(api: PluginAPI) {
    api.internal.registerInternalService(
      'fsService',
      new FilesystemService()
    )
    // Now api.fs.read(), api.fs.write(), api.fs.list(), api.fs.watch() work
  }
}
```

**Provides:** `api.fs.read()`, `api.fs.write()`, `api.fs.list()`, `api.fs.watch()`, `api.fs.unwatch()`

---

### core.theme-service

Manages the theme registry and the CSS token store. Switching themes = swapping CSS custom property values on the root element.

**Provides:** `api.themes.register()`, `api.themes.activate()`, `api.themes.current()`

---

### core.language-service

Manages the language registry: file extension → language ID mappings, syntax grammar registrations.

**Provides:** `api.languages.register()`, `api.languages.getForFile()`

---

## UI Plugins

### core.title-bar

Renders the custom window title bar with Tauri window controls (minimize, maximize, close). Uses `data-tauri-drag-region` for native window dragging.

```typescript
export const titleBarPlugin: Plugin = {
  manifest: {
    id: 'core.title-bar',
    name: 'Title Bar',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
  },

  activate(api: PluginAPI) {
    api.views.register('titleBar', {
      slot: 'titleBar',
      component: TitleBarView,
      priority: 0,
    })
  }
}
```

**TitleBarView** renders:
- App icon and name (left)
- `data-tauri-drag-region` spanning the full width
- Minimize / Maximize / Close buttons calling Tauri's `appWindow` API (right)

---

### core.activity-bar

Renders the left icon strip. Reads registered activity bar items from the slot and renders them as icon buttons. Activating an icon sets `activityBar.activeItem` in the layout store, which the sidebar plugin reads to show the correct view.

```typescript
activate(api: PluginAPI) {
  api.views.register('activityBar', {
    slot: 'activityBar',
    component: ActivityBarView,
    priority: 0,
  })
}
```

**ActivityBarView** reads from a dedicated `activityBarStore` that other plugins populate:

```typescript
// Other plugins add items like:
api.activityBar.addItem({
  id: 'fileExplorer',
  icon: 'files',
  title: 'Explorer',
  viewId: 'fileExplorer',
  priority: 10,
})
```

---

### core.command-palette

Registers into the overlay slot. Always mounted. Reads from the command registry to populate its list. Visible when `commandPaletteVisible` context key is `true`.

```typescript
activate(api: PluginAPI) {
  api.views.register('commandPalette', {
    slot: 'overlay',
    component: CommandPaletteView,
    priority: 100,
  })

  api.commands.register('workbench.action.showCommandPalette', () => {
    api.context.set('commandPaletteVisible', true)
  })
}
```

**CommandPaletteView:**
- Reads `useCommandRegistry(s => s.all())` for the full command list
- Filters by query string
- Shows keybinding hint per command
- Executes on Enter or click
- Dismisses on Escape or backdrop click
- Focus trapped while open

---

### core.settings

Registers into the overlay slot. Reads from the configuration registry to auto-generate all settings sections.

```typescript
activate(api: PluginAPI) {
  api.views.register('settings', {
    slot: 'overlay',
    component: SettingsPanelView,
    priority: 90,
  })

  api.commands.register('workbench.action.openSettings', () => {
    api.context.set('settingsPanelVisible', true)
  })
}
```

**SettingsPanelView:**
- Nav sidebar listing all registered config sections (sorted by `order`)
- Search bar that filters across all sections
- Auto-generates controls from schema types (`boolean` → checkbox, `select` → dropdown, etc.)
- Reads/writes to configStore — changes are immediate and reactive

---

### core.notifications-ui

Renders notification toasts. Reads from the notification queue.

```typescript
activate(api: PluginAPI) {
  api.views.register('notificationsUi', {
    slot: 'overlay',
    component: NotificationContainer,
    priority: 200,  // higher priority = above other overlays
  })
}
```

**NotificationContainer:**
- Subscribes to the notification queue
- Renders toast stack (bottom-right by default)
- Handles auto-dismiss timers
- Renders action buttons if provided

---

## Feature Plugins

### core.file-explorer

Contributes the file tree sidebar view, file CRUD commands, and context menu entries.

```typescript
export const fileExplorerPlugin: Plugin = {
  manifest: {
    id: 'core.file-explorer',
    name: 'File Explorer',
    dependsOn: ['core.filesystem-service', 'core.sidebar'],
    contributes: {
      commands: [
        { id: 'fileExplorer.newFile',    title: 'New File',    category: 'File' },
        { id: 'fileExplorer.newFolder',  title: 'New Folder',  category: 'File' },
        { id: 'fileExplorer.deleteFile', title: 'Delete File', category: 'File' },
        { id: 'fileExplorer.renameFile', title: 'Rename File', category: 'File' },
      ],
      configuration: {
        pluginId: 'core.file-explorer',
        title: 'File Explorer',
        order: 10,
        schema: [
          {
            key: 'fileExplorer.showHidden',
            title: 'Show hidden files',
            type: 'boolean',
            default: false,
            description: 'Show files and folders starting with a dot',
          },
          {
            key: 'fileExplorer.sortOrder',
            title: 'Sort order',
            type: 'select',
            options: ['name', 'modified', 'created'],
            default: 'name',
            description: 'How to order files in the tree',
          },
        ]
      }
    }
  },

  activate(api: PluginAPI) {
    api.views.register('fileExplorer', {
      slot: 'sidebar',
      component: FileExplorerView,
      priority: 10,
    })

    api.activityBar.addItem({
      id: 'fileExplorer',
      icon: 'files',
      title: 'Explorer',
      viewId: 'fileExplorer',
      priority: 10,
    })

    api.commands.register('fileExplorer.newFile', async () => {
      const name = await api.input.prompt('File name:')
      if (name) await api.fs.write(name, '')
    })

    // ... other command handlers
  }
}
```

---

### core.terminal

Contributes a terminal panel using xterm.js + Tauri's shell spawn API.

```typescript
export const terminalPlugin: Plugin = {
  manifest: {
    id: 'core.terminal',
    name: 'Terminal',
    dependsOn: ['core.panel-area'],
    contributes: {
      commands: [
        { id: 'terminal.new',    title: 'New Terminal' },
        { id: 'terminal.toggle', title: 'Toggle Terminal' },
      ],
      keybindings: [
        { command: 'terminal.toggle', key: 'ctrl+`' },
      ],
      configuration: {
        pluginId: 'core.terminal',
        title: 'Terminal',
        order: 30,
        schema: [
          {
            key: 'terminal.shell',
            title: 'Shell path',
            type: 'string',
            default: '',
            description: 'Path to shell binary. Leave empty for system default.',
          },
          {
            key: 'terminal.fontSize',
            title: 'Font size',
            type: 'number',
            default: 13,
            description: 'Terminal font size in pixels',
          },
        ]
      }
    }
  },

  activate(api: PluginAPI) {
    api.views.register('terminal', {
      slot: 'panelArea',
      component: TerminalView,
      priority: 10,
    })

    api.commands.register('terminal.toggle', () => {
      const visible = api.context.get('panelAreaVisible')
      api.context.set('panelAreaVisible', !visible)
    })
  }
}
```

---

## Curated Default-On Set (WI-43)

The shell binary ships 38 built-in plugins but the boot path does not register them all unconditionally. `shell/src/plugins/catalog.ts` splits them into two buckets:

- **`DEFAULT_ON_PLUGINS`** (19) — loaded at every boot. Core services, workspace + git, chrome slots, files/editor/outline, command palette, confirm, pane mode, search, and `pluginsMgmt` (which you need to enable the rest).
- **`DEFAULT_OFF_PLUGINS`** (17) — shipped but dormant. AI, agent, MCP, workflow, skills, terminal, processes, graph (+ global index), canvas, bases, backlinks, bookmarks, outgoing links, file properties, tags, all properties.

The user opts into a default-off plugin via **Settings > Plugins**: the "Available (disabled)" section renders one row per dormant plugin with an Enable button. Enable writes the plugin's id into the persisted `plugins.enabled: string[]` config value (via `api.configuration.setValue`, backed by the same `configStore` pathway every other config key uses) and notifies the user to reload the window. On next boot, `main.tsx` composes the registered set as `[...DEFAULT_ON_PLUGINS, ...DEFAULT_OFF_PLUGINS.filter(p => enabledIds.has(p.manifest.id))]`.

Rationale: this is a personal tool. The full 38-plugin boot set was noisy and most feature surfaces (AI, agents, graph, canvas, bases) are occasional-use. Curating default-on to the note-taking core reduces own-dogfood friction without removing any capability — every default-off plugin is one click + one reload away.

No plugin is ever deleted by disablement. The grep acceptance guard (`grep -c "^import.*Plugin" shell/src/plugins/catalog.ts` == 38) ensures every built-in stays on disk even when the default-on list shrinks.
