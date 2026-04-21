# Architecture Overview

## The Fundamental Principle

The shell is a substrate, not an application. It provides surfaces (slots) that plugins render into, services that plugins consume, and a communication layer (event bus) that plugins use to talk to each other. Without plugins, the shell renders nothing.

This architecture has one invariant: **the shell never hardcodes UI**. Every visible element — title bar, sidebar, editor, status bar, command palette — is contributed by a plugin at runtime.

---

## The Four Layers

```
Layer 4: Feature plugins
         git, search, AI assistant, linters, formatters
         depends on: service plugins + UI plugins
              ↑
Layer 3: UI plugins
         file explorer, command palette, settings panel,
         notification toasts, theme picker, terminal
         depends on: service plugins
              ↑
Layer 2: Service plugins
         configuration, filesystem, notifications,
         themes, languages
         depends on: shell substrate only
              ↑
Layer 1: Shell substrate
         slot surfaces, extension host, plugin registry,
         context key service, event bus
         no dependencies — this is the foundation
```

Each layer depends only on the layers above it. This is enforced by the extension host's dependency resolution: a plugin will not activate until all its declared dependencies have activated.

---

## Layer 1: Shell Substrate

The shell substrate is what the binary ships. It has no UI of its own. It provides:

### Slot Surfaces

Named regions in the React tree where plugins can render UI. The shell defines the spatial structure but renders nothing in the slots until plugins fill them.

```
overlay          — floating UI above everything (modals, palettes, dialogs)
titleBar         — top of window
activityBar      — far-left icon strip
sidebar          — collapsible left panel
editorArea       — the main editing workspace
panelArea        — collapsible bottom panel
statusBarLeft    — left side of status bar
statusBarRight   — right side of status bar
```

### Extension Host

Responsible for loading plugins, calling their `activate()` functions in dependency order, and cleaning up when a plugin is unloaded. See [Extension Host](extension-host.md).

### Plugin Registry

A collection of sub-registries, one per contribution type. Plugins register into these; the shell reads from them. See [Registry System](registry-system.md).

### Context Key Service

A key-value store of application state. Used to evaluate `when` conditions on commands and keybindings, and to drive conditional rendering in plugin UI. See [Context Keys](context-keys.md).

### Event Bus

Typed publish/subscribe for decoupled communication between plugins. Plugins never import each other directly — they communicate through events. See [Event Bus](event-bus.md).

---

## Layer 2: Service Plugins

Service plugins bootstrap infrastructure that other plugins depend on. They typically register nothing visible — no slots, no commands the user sees. Their job is to call `api.internal.registerInternalService()` and make that service available through the plugin API.

### Why service plugins, not built-ins?

If services were hardcoded into the shell, the shell would need to know about all possible services upfront. The service plugin model means the shell stays minimal and services can be swapped — replace `core.configuration-service` with a custom implementation that stores config in a database instead of localStorage.

### Core service plugins

| Plugin ID | Service provided | API surface added |
|---|---|---|
| `core.configuration-service` | ConfigurationRegistry + ConfigStore | `api.configuration` |
| `core.notification-service` | NotificationQueue | `api.notifications` |
| `core.filesystem-service` | Tauri fs abstraction | `api.fs` |
| `core.theme-service` | ThemeRegistry + token store | `api.themes` |
| `core.language-service` | LanguageRegistry | `api.languages` |

---

## Layer 3: UI Plugins

UI plugins consume services and render into slots. They are the visible face of the shell.

Each UI plugin follows the same pattern:
1. Declare dependencies in manifest
2. Register a component into a slot via `api.views.register()`
3. Register commands and keybindings that drive their visibility
4. Use context keys for show/hide rather than dynamic mount/unmount

### Core UI plugins

| Plugin ID | Slot | Depends on |
|---|---|---|
| `core.title-bar` | `titleBar` | — |
| `core.activity-bar` | `activityBar` | — |
| `core.sidebar` | `sidebar` | — |
| `core.editor-area` | `editorArea` | — |
| `core.panel-area` | `panelArea` | — |
| `core.status-bar` | `statusBarLeft`, `statusBarRight` | — |
| `core.command-palette` | `overlay` | — |
| `core.settings` | `overlay` | `core.configuration-service` |
| `core.notifications-ui` | `overlay` | `core.notification-service` |
| `core.theme-picker` | `overlay` (via settings) | `core.theme-service`, `core.configuration-service` |

---

## Layer 4: Feature Plugins

Feature plugins add capabilities on top of the UI and service infrastructure. They contribute commands, views, config sections, and event handlers.

### Core feature plugins

| Plugin ID | What it adds | Depends on |
|---|---|---|
| `core.file-explorer` | Sidebar tree view, file CRUD commands | `core.filesystem-service`, `core.sidebar` |
| `core.terminal` | Terminal in panel area | `core.panel-area` |
| `core.search` | Search sidebar view + commands | `core.filesystem-service`, `core.sidebar` |

---

## Data Flow

### Rendering flow

```
Plugin calls api.views.register(viewId, { slot, component, priority })
    ↓
SlotRegistry adds entry to slot's sorted list
    ↓
Zustand store update triggers re-render
    ↓
App.tsx SlotSurface for that slot re-renders
    ↓
Plugin's component is mounted in the slot
```

### Command execution flow

```
User presses keybinding (or clicks menu item, or calls api.commands.execute())
    ↓
KeybindingRegistry matches chord
    ↓
ContextKeyService evaluates 'when' expression
    ↓
CommandRegistry.execute(commandId, ...args)
    ↓
Handler function runs
    ↓
Handler updates context keys / calls other APIs / emits events
    ↓
Subscribed components re-render
```

### Plugin communication flow

```
Plugin A calls api.events.emit('file:opened', { path })
    ↓
EventBus routes to all subscribers of 'file:opened'
    ↓
Plugin B's handler runs (it subscribed during activate())
    ↓
Plugin B updates its own state
```

---

## The Empty Shell Guarantee

The most important architectural property: **commenting out all plugins from `main.tsx` produces a blank window with no errors**. The shell does not assume any plugin is loaded. Every slot check is "render what's registered, which may be nothing." This is the proof that the architecture is correct.

---

## Key Design Decisions

### Why Zustand for the slot registry?

The slot registry needs to be reactive — when a plugin registers a component, the shell should re-render that slot immediately without a page reload. Zustand gives reactive updates with minimal boilerplate and works outside React (the extension host needs to write to it).

### Why context keys instead of React state for modal visibility?

Context keys are readable by any plugin. If plugin A wants to know whether the command palette is open, it reads `commandPaletteVisible` from the context key service. If it used local React state inside the palette component, that state would be invisible to other plugins. Context keys are the shared ambient state layer.

### Why always-mounted overlay components?

Overlay components (command palette, settings panel) register once at plugin load time and stay mounted, rendering `null` when invisible. The alternative — dynamically mounting/unmounting on open/close — means the component loses its state between sessions, has a brief mounting delay when first opened, and requires a portal injection mechanism. Always-mounted components have none of these problems.

### Why manifest + imperative hybrid?

The manifest declares static contributions that the extension host can register before any plugin code runs — this means the command palette can be populated before every plugin has fully activated. The imperative `activate()` wires the handlers and registers anything that requires runtime logic (conditional contributions, dynamic content). Pure manifest-first would be too rigid; pure imperative would prevent lazy loading.
