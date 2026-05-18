# Commands and keybindings

Every action a user can invoke is a **command** with a string id. The
command palette lists them, keybindings trigger them, menus point to
them, plugins call them. Anchoring everything on commands keeps the
contribution model uniform.

## Register a command

```ts
ctx.commands.register({
  id: 'hello.sayHi',
  title: 'Hello: Say Hi',
  run: async () => {
    ctx.ui.notify({ message: 'Hi!' });
  },
});
```

Conventions:

- **`id`**: `<plugin-namespace>.<verb>` — globally unique. The
  loader rejects duplicate ids.
- **`title`**: starts with the plugin name, then a colon, then the
  human-readable action. The palette groups by plugin name.
- **`run`**: sync or async. Throws are logged and surfaced as a
  toast.

## Default keybinding

Declare in the manifest:

```json
"contributes": {
  "commands": [
    { "id": "hello.sayHi", "title": "Hello: Say Hi", "keybinding": "Ctrl+Shift+H" }
  ]
}
```

Or platform-specific:

```json
"keybinding": { "default": "Ctrl+Shift+H", "mac": "Cmd+Shift+H" }
```

Default keybindings are *suggestions*. The user can override them in
**Settings → Keybindings**; user settings always win.

Pick keybindings that are easy to remember **and unlikely to collide**.
The shell logs conflicts; the later registration wins. Avoid:

- Single-key bindings (they break in input fields).
- Common system shortcuts (`Ctrl+C`, `Ctrl+S`, `Ctrl+Z`).
- The handful reserved by the shell (`Ctrl+Shift+P`, `Ctrl+P`,
  `Ctrl+Shift+F` etc.).

## Chord bindings

A chord is two keystrokes pressed in sequence:

```json
"keybinding": "Ctrl+K Ctrl+H"
```

Press `Ctrl+K`, release, press `Ctrl+H` within the chord timeout
(~1s). Useful for keeping top-level keys uncluttered. Familiar
pattern from VS Code; reach for it when a single chord would collide.

## Conditional bindings — `when` clauses

A binding can be scoped to a context:

```json
{
  "id": "hello.formatBlock",
  "keybinding": "Tab",
  "when": "editor.hasActiveTab && !editor.completionVisible"
}
```

Same key can do different things in different contexts without
conflicting. Full grammar and built-in keys:
[Context keys](context-keys.md).

## Commands without keybindings

Plenty of commands shouldn't have a keybinding — they live in the
palette, in menus, or are called by other plugins:

```json
{ "id": "hello.cleanup", "title": "Hello: Clean Up Greetings" }
```

The palette is enough.

## Menus

Place commands into menus:

```json
"contributes": {
  "menus": [
    { "command": "hello.sayHi", "menu": "editor.context", "group": "navigation" },
    { "command": "hello.cleanup", "menu": "command.palette" }
  ]
}
```

Menu ids:

| Menu | Where it appears |
|---|---|
| `command.palette` | The command palette (default — most commands) |
| `editor.context` | Right-click in the editor |
| `editor.title` | The editor tab's `…` menu |
| `view.title` | A panel header's `…` menu |
| `explorer.context` | Right-click in the file tree |

`group` controls ordering within a menu (alphabetical within a
group).

## Invoking commands

From your own code:

```ts
await ctx.commands.invoke('hello.sayHi');
await ctx.commands.invoke('hello.greetByName', { name: 'Ada' });
```

From the user's perspective: command palette, keybinding, or menu.

## Disable / hide commands

Use `when` clauses on the command itself, not just the binding:

```json
{
  "id": "hello.adminAction",
  "title": "Hello: Admin Action",
  "when": "config.hello.adminMode"
}
```

When the clause is false, the command is hidden from the palette and
its keybinding is inert.

## Removing a command

```ts
const handle = ctx.commands.register({ id: 'hello.sayHi', run: /* … */ });

handle.dispose();
```

Or wait for `deactivate` — the kernel auto-removes registrations.

## Listing commands

```ts
const cmds = ctx.commands.list();
// [{ id, title, when?, keybinding? }, …]
```

Useful for building your own command picker.

## See also

- [Context keys](context-keys.md) — `when` clauses.
- [`../../help/customize/keybindings.md`](../../help/customize/keybindings.md)
  — user-facing keybinding overview.
- [Manifest](../plugins/manifest.md) — `contributes.commands` and
  `contributes.menus` schema.
