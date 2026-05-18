# Editor extension model

The editor (`com.nexus.editor`) is a CodeMirror 6 host with a custom
markdown mode. Plugins extend it through three contribution surfaces:

1. **Slash commands** — `/foo` actions in the editor. See
   [Slash commands](slash-commands.md).
2. **MDX components** — JSX-style tags rendered inside markdown. See
   [MDX components](mdx-components.md).
3. **Decorations** — overlaid widgets, marks, and gutters.

Beyond these three, you can call any of the editor plugin's IPC
commands (open files, insert at cursor, jump to a line) — see
[IPC](../plugins/ipc.md).

## Where extensions run

Editor extensions run in the **shell process**, alongside the
CodeMirror instance. WASM community plugins can register decorations
and slash commands; iframe-JS plugins can do the same plus contribute
MDX components that render React (or other framework) UI.

Core editor internals live in
[`crates/nexus-editor/`](../../../crates/nexus-editor/) and
[`shell/src/plugins/nexus/editor/`](../../../shell/src/plugins/nexus/editor/).

## Decorations

A decoration overlays the document without changing the underlying
text. Three flavors:

| Kind | Use it for |
|---|---|
| **Mark** | Inline span styling — highlight a range, underline a typo. |
| **Widget** | A non-text UI element inserted into the flow — a "run code" button next to a code block. |
| **Block** | A whole-line treatment — change background, add a margin handle. |

Register through the editor IPC:

```ts
const handle = await ctx.ipc.call('com.nexus.editor', 'decorate', {
  path: 'README.md',
  range: { from: 100, to: 120 },
  kind: 'mark',
  className: 'my-plugin-highlight',
});

// Later — clear it
await ctx.ipc.call('com.nexus.editor', 'undecorate', { handle });
```

Decorations are scoped to a file and are recomputed when the document
changes. If your decoration depends on parsed structure (e.g. wraps
every callout block), subscribe to `editor:change` and recompute on
debounce.

## Slash commands

Slash commands let users invoke an action with `/foo` at the start of
a line. Full reference: [Slash commands](slash-commands.md).

Quick form:

```ts
ctx.ipc.call('com.nexus.editor', 'register_slash_command', {
  id: 'hello.insertGreeting',
  trigger: 'greet',
  title: 'Insert greeting',
  description: 'Drop a "Hello, world" line.',
  insertText: 'Hello, world\n',
});
```

## MDX components

Plugins contribute JSX-style components that render inside markdown:

```mdx
<Callout type="warning">
  Don't deploy on Friday.
</Callout>
```

Full reference: [MDX components](mdx-components.md).

## Block model

Every block in a markdown document has a stable ID. The block tree is
the canonical structure your decorations and components attach to. If
you need to operate on "the third paragraph", get its block ID once
and store that — line numbers shift as the user edits.

```ts
const blocks = await ctx.ipc.call('com.nexus.editor', 'list_blocks', {
  path: 'README.md',
});
// blocks: [{ id, kind, range, properties }]
```

## Live preview pipeline

When the user is in live-preview mode, the editor:

1. Parses with [Lezer](https://lezer.codemirror.net) (incremental).
2. Walks the parse tree to compute decorations.
3. Asks contributors (your plugin) for any decorations that depend on
   parsed structure.
4. Applies the decoration set as a single transaction.

Steps (3) and (4) are debounced; you don't get called on every
keystroke. If the parser hasn't caught up to the cursor, partial
decorations render and refine on the next tick.

## Performance

The editor handles documents up to a few MB before slowdown. If your
extension scales with document size:

- **Limit visible ranges.** CodeMirror provides the visible viewport;
  decorate only what's on screen.
- **Cache derived state by block ID**, not range.
- **Debounce expensive work** off the keystroke path.

## When editor extensions are the wrong tool

If you're building a panel that *displays* derived information about
the current note (outline, table of contents, comment list), do it as
a **view** (see [Views and slots](../ui/views-and-slots.md)) and
subscribe to `editor:change`. Editor extensions are for things that
need to live *inside* the document.

## Read next

- [Slash commands](slash-commands.md)
- [MDX components](mdx-components.md)
- [Views and slots](../ui/views-and-slots.md)
