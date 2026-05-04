# Slash commands

A slash command is invoked when the user types `/<trigger>` at the
start of a line. The editor opens a fuzzy menu of matching commands;
selecting one runs your handler.

## Register a static command

The simplest form: a fixed insertion.

```ts
activate(ctx: PluginContext) {
  ctx.ipc.call('com.nexus.editor', 'register_slash_command', {
    id: 'hello.insertGreeting',
    trigger: 'greet',
    title: 'Insert greeting',
    description: 'Drop a "Hello, world" line.',
    icon: 'message-circle',
    insertText: 'Hello, world\n',
  });
}
```

The user types `/greet`, picks the entry, and `Hello, world\n`
replaces the slash sequence at the cursor.

## Dynamic command (custom handler)

For anything more interesting than a fixed insert, register a
callable handler:

```ts
ctx.ipc.handle('hello.runSlashGreeting', async (args: {
  path: string;
  range: { from: number; to: number };
}) => {
  const name = await ctx.ui.prompt({ message: 'Name?' });
  return {
    edits: [
      {
        path: args.path,
        range: args.range,
        text: `Hello, ${name}!\n`,
      },
    ],
  };
});

ctx.ipc.call('com.nexus.editor', 'register_slash_command', {
  id: 'hello.greetByName',
  trigger: 'greet-by-name',
  title: 'Greet by name',
  description: 'Prompt for a name, then insert.',
  handler: 'hello.runSlashGreeting',
});
```

The editor calls back into your handler with the selected `path` and
the `range` to replace (the slash sequence). Return an `edits` array
and the editor applies them as one undo unit.

## Registration fields

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | yes | Unique within your plugin (`<plugin>.<name>`). |
| `trigger` | string | yes | The text after `/` that matches. Letters, digits, hyphens. |
| `title` | string | yes | Shown in the menu. |
| `description` | string | no | Subtitle in the menu. |
| `icon` | string | no | Lucide icon name. |
| `insertText` | string | conditional | Static replacement text. |
| `handler` | string | conditional | IPC command id to call for dynamic behavior. |
| `keywords` | string[] | no | Extra strings the fuzzy matcher should match against. |
| `group` | string | no | Group label in the menu (e.g. "AI", "Lists"). |

Provide either `insertText` (static) or `handler` (dynamic), not
both.

## Fuzzy matching

The slash menu does fuzzy matching against `trigger`, `title`, and
`keywords`. `/grt` matches `greet` and `greet-by-name`. The menu
ranks by match quality; ties break alphabetically by `title`.

## Removing a command

```ts
const handle = await ctx.ipc.call('com.nexus.editor', 'register_slash_command', /* … */);

// Later — typically in deactivate
await ctx.ipc.call('com.nexus.editor', 'unregister_slash_command', { handle });
```

The kernel auto-removes registrations on `deactivate`, so explicit
unregistration is only needed if you're toggling commands at runtime.

## Trigger collisions

If two plugins register the same `trigger`, the editor logs a
warning and shows both in the menu (disambiguating by plugin id).
Pick triggers thoughtfully — `/note`, `/code`, `/list` are all going
to collide.

A planned snippet-style collision detector (see
`SnippetRegistry.getConflicts` for the model) will surface conflicts
in the Plugins panel.

## Patterns

### "Insert with arguments" via prompt

```ts
ctx.ipc.handle('myplugin.calloutSlash', async (args) => {
  const type = await ctx.ui.pick({
    message: 'Callout type',
    options: ['note', 'tip', 'warning', 'danger'],
  });
  return {
    edits: [{
      path: args.path,
      range: args.range,
      text: `> [!${type}]\n> `,
    }],
  };
});
```

### Slash command that opens a panel

```ts
ctx.ipc.handle('myplugin.openSearchSlash', async (args) => {
  await ctx.ipc.call('com.nexus.shell', 'reveal_view', {
    viewId: 'myplugin.searchPanel',
  });
  return { edits: [] };  // no edit; we just opened a panel
});
```

Returning empty `edits` is fine — the slash is just a launcher.

## Performance

Slash command lookup runs on every `/` typed. Don't do work in
`register_slash_command`; the editor caches the registration. Your
handler runs only on selection.

## See also

- [Editor overview](overview.md)
- [Commands and keybindings](../ui/commands-and-keybindings.md) —
  palette commands have similar shape but different invocation.
