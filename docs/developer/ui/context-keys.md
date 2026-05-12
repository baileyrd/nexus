# Context keys and `when` clauses

A **context key** is a named piece of application state that
plugins can read and use to scope keybindings, command visibility,
and conditional UI. The shell publishes core keys; plugins can
publish their own.

Authoritative reference (built-in keys + impl notes):
[`../../shell/context-keys.md`](../../shell/context-keys.md).

## Built-in keys

| Key | Type | True when |
|---|---|---|
| `editor.hasActiveTab` | bool | An editor tab is open |
| `editor.activeTabDirty` | bool | Active tab has unsaved changes |
| `editor.activeTabPath` | string | Path of the active editor tab (or empty) |
| `editor.activeTabLanguage` | string | `markdown`, `canvas`, `bases`, etc. |
| `editor.completionVisible` | bool | An autocomplete popup is open |
| `editor.selectionEmpty` | bool | No selection (cursor only) |
| `view.activeId` | string | id of the focused view |
| `sidebar.visible` | bool | Sidebar is open |
| `terminal.activeSessionId` | string | Active terminal session id |
| `workspace.forgePath` | string | Current forge root |
| `config.<key>` | matches type | Any setting value, e.g. `config.hello.greeting` |

`config.<key>` works for every plugin's settings, so you can scope a
binding by your own setting:

```json
"when": "config.hello.adminMode"
```

## Grammar

A `when` expression is a small boolean DSL:

```
expr      := atom
          |  '!' expr                       -- not
          |  expr '&&' expr                 -- and
          |  expr '||' expr                 -- or
          |  '(' expr ')'

atom      := key                            -- truthy check
          |  key '==' literal               -- equality
          |  key '!=' literal               -- inequality
          |  key '=~' /regex/               -- regex match (string keys)

literal   := 'string' | 123 | true | false | null
```

Examples:

```
editor.hasActiveTab
!sidebar.visible
editor.hasActiveTab && editor.activeTabLanguage == 'markdown'
config.hello.style == 'shouted' || config.hello.style == 'formal'
editor.activeTabPath =~ /^projects\//
```

Operator precedence (highest to lowest): `!`, `==` / `!=` / `=~`,
`&&`, `||`. Use parentheses when in doubt.

## Reading context keys

```ts
const isEditing = ctx.context.get('editor.hasActiveTab') as boolean;
const path = ctx.context.get('editor.activeTabPath') as string;
```

To react to changes:

```ts
const off = ctx.context.subscribe(['editor.hasActiveTab'], (changed) => {
  refreshUi();
});
```

## Publishing your own context keys

Useful when you have UI state that other plugins (or your own
keybindings) should react to:

```ts
ctx.context.set('hello.dialogOpen', false);

// Later
ctx.context.set('hello.dialogOpen', true);
```

Convention: namespace under your plugin id (`hello.*`,
`com.example.foo.*`).

To bind to your own key:

```json
{
  "id": "hello.confirmDialog",
  "keybinding": "Enter",
  "when": "hello.dialogOpen"
}
```

## When `when` clauses apply

| Place | Effect when false |
|---|---|
| `keybinding` `when` | Key sequence falls through to the next handler |
| `command` `when` | Command hidden from the palette |
| `menu` entry `when` | Menu entry hidden |
| `view` `when` | View hidden from sidebar / panel list |

## Performance

Context keys are cheap to read but every change to a key re-evaluates
every active `when` expression. Don't fire `context.set` on every
keystroke — debounce, or model the state as a setting instead.

## Patterns

### Mode-specific commands

```json
{
  "id": "myplugin.toggleZenMode",
  "title": "Zen Mode: Toggle",
  "keybinding": "Ctrl+K Z"
},
{
  "id": "myplugin.exitZen",
  "keybinding": "Escape",
  "when": "myplugin.zenMode"
}
```

`Escape` only does something when zen-mode is on; otherwise it falls
through to other handlers.

### Path-scoped commands

```json
"when": "editor.activeTabPath =~ /\\.md$/"
```

Only show the command when editing markdown.

### Cross-plugin coordination

```json
"when": "ai.streamActive && !ai.streamMine"
```

Show a "Cancel other agent" button only while another plugin is
streaming.

## See also

- [`../../shell/context-keys.md`](../../shell/context-keys.md)
  — full built-in key list with publish locations.
- [Commands and keybindings](commands-and-keybindings.md) — where
  `when` clauses are most often used.
