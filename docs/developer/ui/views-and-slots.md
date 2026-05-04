# Views, panels, and slots

The desktop shell is structured as **slots** (named regions of the
window) into which plugins contribute **views** (panel content). The
shell never renders content of its own — every visible element is a
view contributed by a plugin.

## Slots

| Slot | Where it appears | Typical content |
|---|---|---|
| `activityBar` | Far-left vertical icon strip | Plugin icons that toggle sidebar panels |
| `sidebar` | Collapsible left pane | File tree, search, backlinks, plugin panels |
| `editor` | Main center area | Editor tabs (managed by `com.nexus.editor`) |
| `rightPanel` | Collapsible right pane | Properties, comments, outline |
| `bottomPanel` | Collapsible bottom pane | Terminal, problems, output |
| `statusBar` | Bottom strip | Token count, branch, file info |
| `commandPalette` | Floating overlay | Palette suggestions (managed by shell) |
| `modal` | Floating overlay | Dialogs (managed by shell) |
| `notification` | Floating overlay | Toasts (managed by shell) |

Authoritative reference:
[`../../../shell/docs/slot-system.md`](../../../shell/docs/slot-system.md).

## Contribute a view

Declare statically in the manifest:

```json
"contributes": {
  "views": [
    {
      "id": "hello.panel",
      "title": "Hello",
      "icon": "smile",
      "slot": "sidebar"
    }
  ]
}
```

Then in `activate`:

```ts
ctx.views.register({
  id: 'hello.panel',
  render: (container: HTMLElement) => {
    container.innerHTML = `<h1>Hello, world</h1>`;
    return () => {
      // Cleanup — remove listeners, etc. Called on view close.
    };
  },
});
```

The `render` callback receives a host-provided `HTMLElement` to fill.
Returning a cleanup function is optional but encouraged — the shell
calls it when the view is closed or unloaded.

## Activation

A view-bearing plugin should activate `onView:<id>`:

```json
"activation": ["onView:hello.panel"]
```

The plugin loads when the user first opens the panel, not at startup.

## Status-bar items

Status-bar items are tiny views in the `statusBar` slot:

```ts
const item = ctx.statusBar.add({
  id: 'hello.counter',
  alignment: 'right',
  text: 'Greetings: 0',
  tooltip: 'Number of times you said hi',
  command: 'hello.sayHi',
});

// Update later
item.update({ text: `Greetings: ${count}` });

// Remove
item.dispose();
```

`alignment: 'left'` and `'right'` group items at each end. `command`
is optional — clicking the item runs that palette command.

## Activity-bar items

Activity-bar items are icon entries in the `activityBar` slot.
Clicking one toggles a sidebar panel:

```json
"contributes": {
  "views": [
    { "id": "hello.panel", "title": "Hello", "icon": "smile",
      "slot": "sidebar", "activityBar": true }
  ]
}
```

`activityBar: true` adds the panel's icon to the activity bar
automatically. Without it, the view is reachable only through the
command palette (`Show view: Hello`).

## Headers

Each panel view gets a header with a title, view-specific actions
(arrow, settings, close), and a kebab menu. To add actions:

```ts
ctx.views.register({
  id: 'hello.panel',
  render: (container) => /* … */,
  headerActions: [
    {
      icon: 'refresh-ccw',
      tooltip: 'Refresh',
      command: 'hello.refresh',
    },
  ],
});
```

## Reactive UI

Most plugins want to re-render on data changes. Two patterns:

### Plain DOM with subscriptions

```ts
ctx.views.register({
  id: 'hello.panel',
  render: (container) => {
    const list = document.createElement('ul');
    container.appendChild(list);

    const refresh = async () => {
      const items = await ctx.kv.list('greetings:');
      list.innerHTML = items
        .map((i) => `<li>${escape(i.value)}</li>`)
        .join('');
    };

    refresh();
    const off = ctx.events.subscribe('hello:greeted', refresh);
    return off;
  },
});
```

### React (iframe-JS sandbox only)

```tsx
import { createRoot } from 'react-dom/client';

ctx.views.register({
  id: 'hello.panel',
  render: (container) => {
    const root = createRoot(container);
    root.render(<HelloPanel ctx={ctx} />);
    return () => root.unmount();
  },
});
```

The shell ships React 18 in the iframe-JS runtime. WASM plugins can
bring any framework but pay a download cost — keep it small.

## Pop-out windows

Any view can be popped into its own window. The shell provides this
without plugin involvement (right-click the panel → **Pop out**).
Your `render` callback runs again in the new window's DOM; treat
each invocation as independent.

ADR: [`../../adr/0020-popout-window-architecture.md`](../../adr/0020-popout-window-architecture.md).

## Modals

For transient interactions, prefer **modals** over views:

```ts
const result = await ctx.ui.modal({
  title: 'Pick a name',
  render: (container) => {
    container.innerHTML = `<input id="n" /> <button id="ok">OK</button>`;
    return new Promise<string>((resolve) => {
      container.querySelector('#ok')!.addEventListener('click', () => {
        resolve((container.querySelector('#n') as HTMLInputElement).value);
      });
    });
  },
});
```

The shell handles focus trap, backdrop, and Esc-to-close.

## Layout persistence

The shell remembers which panels are open, their sizes, and which
tab is active across restarts. Storage:
`<forge>/.forge/workspace.json`. You don't manage this — declaring
views in the manifest is enough.

For your own per-view state (scroll position, selection, etc.), use
`ctx.kv` namespaced under your plugin id.

## See also

- [`../../../shell/docs/slot-system.md`](../../../shell/docs/slot-system.md)
  — slot internals.
- [`../../../shell/docs/workspace-layout.md`](../../../shell/docs/workspace-layout.md)
  — `workspace.json` schema.
- [Context keys](context-keys.md) — when-clauses for conditional UI.
