# MDX components

MDX components are JSX-style tags users embed in markdown that render
as host-controlled UI. The editor renders a fixed registry of
component tags; plugins extend the registry.

```mdx
<Callout type="warning">
  Don't deploy on Friday.
</Callout>

<Card title="Status" color="blue">
  - [x] Build green
  - [ ] Tests passing
</Card>
```

## Built-in components

Shipped in the editor plugin:

| Tag | Props | Renders |
|---|---|---|
| `<Callout>` | `type: 'note' \| 'tip' \| 'warning' \| 'danger' \| 'info' \| 'quote'`, `title?` | A bordered colored block. |
| `<Alert>` | `variant: 'info' \| 'warning' \| 'error' \| 'success'`, `title?` | Toast-style inline alert. |
| `<Card>` | `title?`, `color?` | A bordered card. |
| `<Badge>` | `text` | An inline pill. |

Use these freely from any markdown file; no plugin needed.

## Contribute a component

```ts
import type { PluginContext } from '@nexus/extension-api';
import { jsx } from 'react/jsx-runtime';
import * as React from 'react';

activate(ctx: PluginContext) {
  ctx.editor.registerMdxComponent('Tweet', {
    schema: {
      props: {
        id: { type: 'string', required: true },
        hideThread: { type: 'boolean', default: false },
      },
      hasChildren: false,
    },
    render: ({ props }) => (
      <iframe
        src={`https://example.com/tweets/${props.id}`}
        sandbox="allow-scripts"
        loading="lazy"
        style={{ width: '100%', height: 200, border: 0 }}
      />
    ),
  });
}
```

User markdown can now contain:

```mdx
<Tweet id="1234" />
```

## Registration shape

| Field | Required | Meaning |
|---|---|---|
| `schema.props` | yes | Per-prop type, default, required flag. Validated at parse time. |
| `schema.hasChildren` | yes | Whether the component takes a body (`<Tag>...</Tag>` vs `<Tag />`). |
| `render` | yes | A pure function `({ props, children }) => ReactNode`. |
| `displayName` | no | Used in error messages. |
| `editor` | no | Optional in-editor inline-edit UI; see "Editing the component" below. |

Prop types: `string`, `number`, `boolean`, `string[]`, `enum: [...]`.
Anything else needs to come in as a JSON string.

## Children

`hasChildren: true` lets users write markdown inside the body:

```mdx
<Card>
  Markdown **inside** renders normally —
  including [[wikilinks]] and `code`.
</Card>
```

The `children` prop your `render` receives is a React node tree
already containing rendered markdown — wrap it in your own layout but
don't try to re-parse.

Markdown inside a body works for inline elements (bold, italic,
links, code, wikilinks) and block elements (headings, lists, tables).
Layout depends on your component's CSS.

## Validation

If a user writes `<Tweet>` with a missing required prop, the
component renders as a red error block:

```
⚠ <Tweet>: missing required prop "id"
```

The user can hover for the full error and click to position the
cursor at the offending tag.

If a user writes `<UnknownTag />`, it renders as plain literal text
(`<UnknownTag />`) — never a script, never an injection. Unknown tags
never break the document.

## Editing the component

Optionally, components can offer an inline editor:

```ts
ctx.editor.registerMdxComponent('Tweet', {
  schema: { /* … */ },
  render: ({ props }) => /* … */,
  editor: {
    type: 'form',
    fields: [
      { key: 'id', label: 'Tweet ID', type: 'string' },
      { key: 'hideThread', label: 'Hide thread', type: 'boolean' },
    ],
  },
});
```

A pencil icon appears on hover; clicking it opens a popover form
that edits the props in place.

## Performance

`render` runs whenever the host re-renders (typically on prop change
or document edit nearby). Keep it cheap. For heavy renders (charts,
embeds), wrap in `React.memo` and key by stable props.

The editor lazy-mounts components in the visible viewport; offscreen
components don't render until scrolled into view.

## Why MDX, not raw HTML

Raw HTML in markdown would let any document inject anything — XSS,
phishing, fake UI. MDX components are **registered** with the host,
so the set of usable tags in a forge is bounded by the plugins
installed there. Auditing what `<Foo />` can do means looking up
which plugin registered it.

A plugin that wants to render arbitrary HTML still has to do so
through React (sanitized) — there's no escape hatch.

## When to use a component vs. a code-block renderer

If your tag carries data that's clearly *content* (a tweet URL, a
chart spec), use a component. If your tag is *code* the user will
edit (a Mermaid diagram, an SVG path), prefer a fenced code block
with a language tag — the content stays text, the editor renders it
when in preview, and the user has the raw form in source mode.

## See also

- [Editor overview](overview.md)
- [`../../help/editing/embeds-and-mdx.md`](../../help/editing/embeds-and-mdx.md)
  — user-facing overview of embeds and components.
