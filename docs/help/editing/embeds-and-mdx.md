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

Self-closing and block-form JSX tags work as host-approved components.
The editor's MDX extractor (`shell/src/plugins/nexus/editor/cm/mdxComponentExtension`)
scans the document for tags matching `/^[A-Z][A-Za-z0-9]*$/` and
renders each as an inline widget, passing the parsed attributes as
props.

**No components ship built-in today.** Plugins author and register
them; in a fresh forge with only stock plugins, any tag you write
renders as plain text (the deliberate fallback so unknown components
never break a note). What MDX gives you is the *seam* — the host
parses the tag and looks up a registered component; what fills the
seam is up to the plugins you install.

Once a plugin registers (say) a `Card` component, you can use it
like this:

```mdx
<Card title="Status" color="blue">
  Markdown **inside** the body renders normally —
  including [[wikilinks]] and `code`.
</Card>

<MyBadge text="draft" />
```

Props are parsed from the JSX attribute syntax (strings, numbers,
booleans). Markdown inside a component body works for inline elements
(bold, italic, links, code, wikilinks). Block elements (headings,
lists, tables) render too, but layout depends on the component's
declared output.

## Why MDX, not raw HTML?

Raw HTML would let any plugin inject anything into your note.
Components are **registered** with the host, so the set of usable tags
in a forge is bounded by the plugins you've installed. You can audit
what `<Foo />` will do by looking up the component contributor.

The host never evaluates plugin-supplied JSX — the component's
`render(props)` returns a `PanelNode` tree (declarative, host-walked
through the same approved-primitives dispatcher used by panel views),
not arbitrary DOM/HTML. Same trade-off as `registerPanelView`:
approved primitives only, no HTML escape hatch.

## Custom components

Plugins contribute components through the editor surface:

```ts
import type { MdxComponent } from '@nexus/extension-api';

const myCard: MdxComponent = {
  id: 'com.example.cards.card',
  name: 'Card',
  description: 'A titled callout-style card.',
  render: (props) => ({
    kind: 'box',
    children: [
      { kind: 'text', text: String(props.title ?? ''), variant: 'heading' },
      { kind: 'markdown', source: String(props.children ?? '') },
    ],
  }),
};

const dispose = ctx.editor.registerMdxComponent(myCard);
```

`MdxComponent.render(props)` returns a `PanelNode` — the same declarative
primitive used by `registerPanelView`. Names must start uppercase and
match `/^[A-Z][A-Za-z0-9]*$/` per the JSX tag rule. The returned
`Disposable` should be added to `ctx.disposables` so the host can sweep
the registration when the plugin stops.

See [Building your own plugin](../plugins/build-your-own.md) and
`packages/nexus-extension-api/src/index.ts` (search for
`registerMdxComponent`) for the authoritative API.
