# Keybindings and the command palette

Every action in Nexus is a **command** with an ID. Commands can be
invoked from the palette, bound to a keystroke, or called by other
plugins. Plugins contribute commands; you can rebind any command and
add new bindings.

## The command palette

`Ctrl+Shift+P` (or `Cmd+Shift+P` on macOS) opens the palette. Fuzzy
search across every command in the workspace:

- "create note" → **File: Create note**
- "ai chat" → **AI: Open chat**
- "term" → **Terminal: New session**, **Terminal: Run saved command**…

Each entry shows its current keybinding (if any) on the right.

## Default keybindings

A selection — not exhaustive. The command palette is the canonical
list.

| Action | macOS | Linux / Windows |
|---|---|---|
| Command palette | `Cmd+Shift+P` | `Ctrl+Shift+P` |
| Open file (quick switch) | `Cmd+P` | `Ctrl+P` |
| Save file | `Cmd+S` | `Ctrl+S` |
| Find in file | `Cmd+F` | `Ctrl+F` |
| Find in workspace | `Cmd+Shift+F` | `Ctrl+Shift+F` |
| Toggle live preview | `Cmd+Shift+E` | `Ctrl+Shift+E` |
| Inline AI completion | `Cmd+Shift+Space` | `Ctrl+Shift+Space` |
| Comment thread on selection | `Cmd+K Cmd+C` | `Ctrl+K Ctrl+C` |
| Move active tab left | `Cmd+Alt+Left` | `Ctrl+Alt+Left` |
| Move active tab right | `Cmd+Alt+Right` | `Ctrl+Alt+Right` |
| New terminal session | — | `Ctrl+Shift+T` |
| Quit | `Cmd+Q` | `Ctrl+Q` |

## Customize a binding

Open **Settings → Keybindings**. The panel shows every command with its
current binding. Click the binding to record a new one (or `Esc` to
unbind).

CLI:

```bash
nexus config set keybindings."com.nexus.editor.toggleLivePreview" "Ctrl+P"
```

Conflicts are detected — if two commands try to claim the same
keystroke, the later one wins and the earlier one is shown as
**(masked)** in the panel.

## Chord bindings

Multi-key sequences work: `Ctrl+K Ctrl+C` is a chord (press `Ctrl+K`,
release, then `Ctrl+C` within the chord timeout). Chords let you keep
top-level keys uncluttered.

## Context-sensitive bindings

Bindings can be scoped with a `when` clause to a context key:

```json
{
  "command": "editor.acceptCompletion",
  "key": "Tab",
  "when": "editor.completionVisible"
}
```

Context keys are published by plugins (e.g. `editor.hasActiveTab`,
`editor.activeTabDirty`). The same key can do different things in
different contexts without conflicts.

## Plugins contributing commands

A plugin manifest declares its commands:

```json
"contributes": {
  "commands": [
    { "id": "myplugin.doThing", "title": "MyPlugin: Do thing", "keybinding": "Ctrl+Shift+D" }
  ]
}
```

Default keybindings from manifests are suggestions — your user
settings always win.

## Reset

To reset all keybindings to defaults:

```bash
nexus config reset keybindings
```
