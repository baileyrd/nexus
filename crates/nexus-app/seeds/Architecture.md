# Architecture

Forge is an **editor shell** — the same shape as VS Code, IntelliJ,
Eclipse, Obsidian. The application chrome (sidebar, tabs, status
bar, command palette) is a thin core; every panel, view, language
feature, and tool window is a plugin that registers into an
extension point.

## The microkernel

A handful of crates make up the kernel:

| Crate           | Role                                                  |
| --------------- | ----------------------------------------------------- |
| `nexus-kernel`  | Event bus, lifecycle, capability system               |
| `nexus-plugins` | Plugin manifest, loader, RPC bridge                   |
| `nexus-storage` | Forge layout, atomic writes, Tantivy search, SQLite   |
| `nexus-theme`   | Theme engine + layout presets (Obsidian, Dev, …)      |
| `nexus-app`     | Tauri shell + IPC adapters                            |

The kernel knows nothing about markdown, canvases, databases, or
terminals. Those are **plugins** — even the first-party ones.

## Contribution points

Every visible surface is a named contribution:

- **Content types** — `com.nexus.editor.markdown`, `base`,
  `canvas`, `outline`.
- **Panels** — `explorer`, `search`, `bookmarks`, `tags`, `graph`,
  plus anything a plugin registers.
- **Commands** — resolved by the palette and keybindings.
- **Status-bar items** — live counters, sync state, cursor
  position.
- **Themes & layouts** — palette, typography, and ribbon / panel
  arrangements.

## The data flow

```
┌─────────────────┐  IPC  ┌─────────────────┐
│  Tauri webview  │ ───►  │  Plugin process │
│  (React + CM6)  │       │ (Rust or WASM)  │
└────────┬────────┘       └────────┬────────┘
         │                         │
         │      event bus          │
         └─────────────┬───────────┘
                       ▼
               ┌───────────────┐
               │ Forge on disk │
               │ notes/ .forge │
               └───────────────┘
```

Plugins never touch disk directly — every read / write / rename
goes through `com.nexus.storage` so capability checks, atomic
writes, and audit hooks apply uniformly.

## Further reading

- [[Decision log]] — the "why" behind the key calls.
- [[Welcome]] — the shorter pitch.
- [[Quick start]] — the hands-on tour.
