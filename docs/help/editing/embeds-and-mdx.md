# Embeds and MDX components

Beyond plain markdown, Nexus supports two ways to enrich a note:
**embeds** (pull other notes or files in inline) and **MDX components**
(JSX-style component tags rendered by host plugins).

## Embeds

Embeds are wikilinks with a leading `!`. They render the target inline
in live preview and read mode.

| Syntax | What it does |
|---|---|
| `![[image.png]]` | Image, video, audio, PDF — any attachment |
| `![[Other Note]]` | Whole note rendered inline |
| `![[Other Note#Heading]]` | Just that section |
| `![[Other Note#^a1b2c3]]` | Just that block |

Embeds re-render when the target changes. Editing the source updates
every embed of it.

## MDX components

Self-closing or block-form JSX tags work as host-approved components.
Built-in component set: `Card`, `Callout`, `Alert`, `Badge`. Plugins
can register more.

```mdx
<Card title="Status" color="blue">
  Markdown **inside** the body renders normally —
  including [[wikilinks]] and `code`.
</Card>

<Alert variant="warning">Ship freeze starts Friday.</Alert>

<Badge text="draft" />
```

Props are parsed from the JSX attribute syntax. Unknown tags render as
plain text so unknown components never break a note.

Markdown inside a component body works for inline elements (bold,
italic, links, code, wikilinks). Block elements (headings, lists,
tables) render too, but layout depends on the component's CSS.

## Why MDX, not raw HTML?

Raw HTML would let any plugin inject anything into your note.
Components are **registered** with the host, so the set of usable tags
in a forge is bounded by the plugins you've installed. You can audit
what `<Foo />` will do by looking up the component contributor.

## Custom components

Plugins contribute components through the extension API:

```ts
context.editor.registerMdxComponent('MyTag', {
  render: (props, children) => /* return DOM/React */
});
```

See [Building your own plugin](../plugins/build-your-own.md) and
`shell/docs/plugin-api.md` for the full API.
