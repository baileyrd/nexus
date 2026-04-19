# tauri-shell

A generic, plugin-first editor shell built with Tauri 2.0, React, TypeScript, and Zustand.

**The shell starts completely empty.** There is no sidebar, no title bar, no editor, no status bar until plugins load them. Every visible element is contributed by a plugin. The shell binary provides only the substrate: slot surfaces, an extension host, a plugin registry, and a typed event bus.

This is the same fundamental architecture used by VS Code and JetBrains IDEs, implemented from scratch as a clean reference.

---

## Documentation

| Document | Description |
|---|---|
| [Architecture Overview](docs/architecture.md) | The complete mental model — shell substrate, plugin layers, data flow |
| [Plugin System](docs/plugin-system.md) | How plugins are defined, loaded, and unloaded |
| [Extension Host](docs/extension-host.md) | Lifecycle management, dependency ordering, activation events |
| [Registry System](docs/registry-system.md) | All sub-registries: commands, views, slots, config, keybindings |
| [Slot System](docs/slot-system.md) | How the shell renders plugin-contributed UI |
| [Context Keys](docs/context-keys.md) | Application state for command enablement and conditional rendering |
| [Event Bus](docs/event-bus.md) | Typed publish/subscribe between plugins |
| [Plugin API](docs/plugin-api.md) | The full API surface handed to every plugin's activate() |
| [Core Plugins](docs/core-plugins.md) | Service plugins and UI plugins — load order and dependencies |
| [Writing a Plugin](docs/writing-a-plugin.md) | Step-by-step guide with examples |
| [Predefined Layouts](docs/predefined-layouts.md) | Named layout snapshots, switching, persistence |

---

## Quick Start

```bash
# Install dependencies
pnpm install

# Run in development
pnpm tauri dev

# Build
pnpm tauri build
```

---

## The Core Idea

```
Shell binary starts → ExtensionHost loads plugins → plugins register into slots → React renders slots
```

With no plugins loaded, you see a blank window. That is correct and intentional.

Add plugins to `src/main.tsx` to assemble the shell:

```typescript
// Start with nothing
const plugins: Plugin[] = []

// Add core services first
plugins.push(configurationServicePlugin)
plugins.push(notificationServicePlugin)
plugins.push(fileSystemServicePlugin)

// Then UI plugins
plugins.push(activityBarPlugin)
plugins.push(commandPalettePlugin)
plugins.push(settingsPlugin)

// Then feature plugins
plugins.push(fileExplorerPlugin)
plugins.push(terminalPlugin)

await host.loadAll(plugins)
```

Comment out any plugin and that capability disappears. No other code changes.

---

## Architecture in One Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│  Tauri 2.0 Window                                               │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  React root — App.tsx                                     │  │
│  │  Renders ONLY what plugins have registered into slots     │  │
│  │                                                           │  │
│  │  [overlay slot]    ← command palette, settings, dialogs  │  │
│  │  [titleBar slot]   ← title bar                           │  │
│  │  [activityBar slot]← activity bar icons                  │  │
│  │  [sidebar slot]    ← file explorer, search, git          │  │
│  │  [editorArea slot] ← text editors, previews              │  │
│  │  [panelArea slot]  ← terminal, output, problems          │  │
│  │  [statusBar slot]  ← status items                        │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  Extension Host                                           │  │
│  │  Loads plugins · calls activate() · manages lifecycle     │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │  Plugin Registry                                          │  │
│  │  commands · views · menus · keybindings · config · slots  │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```
