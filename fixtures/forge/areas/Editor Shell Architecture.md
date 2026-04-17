---
tags: [architecture, evergreen, editor-shell]
---

# Editor shell architecture

Pair with [[areas/Microkernel Patterns]].

## What "editor shell" means

Look at VS Code, Eclipse, IntelliJ, Obsidian. The **chrome** — the
sidebar, tab strip, status bar, command palette — is a thin host.
Every **panel**, **view**, **language feature**, **tool window**
is a plugin that registers into extension points. The UI
framework serves the core, not the other way around.

Nexus's TUI (ratatui) and desktop app (React + Tauri) both take
this shape. Content-type registration is the common seam.

## The contribution registry

Every UI extension point goes through
`contributions.register<X>` in `app/src/contributions/registry.ts`:

- `registerContentType(id, component)` — what renders for a tab
  whose `contentType` matches `id`.
- `registerCommand(id, handler)` — what runs when the command
  palette or a keybinding triggers `id`.
- `registerPaletteCommand({ commandId, title, keybinding })` — how
  the user discovers the command.
- `registerSettingsTab(...)`, `registerMenuItem(...)`,
  `registerFileHandler(...)` — same pattern, different surface.

Every built-in UI feature is registered through the same API a
community plugin would use. No fast-path. This means we eat our
own dog food — if `registerContentType` isn't good enough for a
built-in, it isn't good enough for anyone.

## Tab dispatch

Tabs carry a `contentType` string:

- `file:<relpath>` — renders `<FileViewer />`.
- `base-file:<relpath>` — renders `<BaseFileView />`.
- `terminal` — renders `<TerminalPanel />`.
- `bases-demo` — the hardcoded demo surface.
- Any registered content-type id — renders the registered
  component.

`PaneView` dispatches based on the prefix. New prefixes are a
two-line change + a component.

## What lives on disk

Everything. Tabs, layout persistence, editor preferences, plugin
settings — all round-tripped through a forge-relative file or the
`.forge/` KV store. The app remembers nothing the user can't
`cat`.

## What I keep getting wrong

- **State in React.** Tempting to put the "active tab" in a React
  store. But then TUI and the CLI can't see it. Put it in
  kernel-level persistence.
- **Registering twice.** Hot reload re-imports modules; registrations
  must be idempotent. `registerContentType` replaces on conflict,
  but `registerCommand` warns — handle both.
- **Forgetting keybindings.** Adding a command without a palette
  entry hides it from discovery. Always pair them.

## Links

- [[areas/Microkernel Patterns]]
- [[projects/Nexus/Overview]]
