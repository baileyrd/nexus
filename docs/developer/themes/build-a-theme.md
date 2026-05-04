# Build a theme

A Nexus theme is a plugin whose only contribution is a CSS variable
overlay. The shell's design system reads ~497 variables; a theme
defines values for some or all of them, and the shell hot-applies the
result.

## Scaffold

```bash
nexus plugin scaffold \
  --template theme \
  --id com.you.midnight \
  --name "Midnight"
cd midnight
```

You get:

```
midnight/
├── plugin.json
├── theme.css            # your CSS variable overrides
└── README.md
```

The manifest is minimal — themes don't need code:

```json
{
  "id": "com.you.midnight",
  "name": "Midnight",
  "version": "1.0.0",
  "apiVersion": 1,
  "sandboxed": true,
  "capabilities": [],
  "contributes": {
    "themes": [
      {
        "id": "midnight",
        "label": "Midnight",
        "type": "dark",
        "css": "theme.css"
      }
    ]
  }
}
```

`type: "dark"` (or `"light"`) tells the shell which side of the
auto-switch to honor when "Auto" theme is selected.

## Minimal `theme.css`

```css
:root[data-nexus-theme="midnight"] {
  /* Background tier */
  --color-background-base:        #0d1117;
  --color-background-surface:     #161b22;
  --color-background-elevated:    #21262d;

  /* Text tier */
  --color-text-primary:           #c9d1d9;
  --color-text-secondary:         #8b949e;
  --color-text-disabled:          #484f58;

  /* Accent tier */
  --color-accent:                 #58a6ff;
  --color-accent-hover:           #79b8ff;
  --color-accent-pressed:         #388bfd;

  /* Border tier */
  --color-border-default:         #30363d;
  --color-border-muted:           #21262d;
}
```

That's the bare minimum — about 12 variables — and it'll already
look like a coherent dark theme. Filling in the remaining ~485
variables refines specific surfaces (selection highlights, scrollbar
colors, callout types, etc.).

Full variable list: [CSS variable reference](css-variables.md).

## Inheriting from a base theme

Don't rewrite the whole stack. Import a base and override only the
deltas:

```css
@import url('nexus-theme://nexus-dark');

:root[data-nexus-theme="midnight"] {
  --color-accent: #58a6ff;
  --color-background-base: #0d1117;
  /* ...just the 5–10 variables that make your theme distinctive */
}
```

Available bases:

- `nexus-theme://nexus-light`
- `nexus-theme://nexus-dark`
- `nexus-theme://high-contrast-light`
- `nexus-theme://high-contrast-dark`

## Test live

The shell hot-reloads theme CSS over the event bus:

```bash
nexus plugin install --watch ./
```

Edit `theme.css`, save — under a frame, the shell re-applies. No
restart required. Use this loop to iterate on every variable until
the look feels right.

## Selectors and specificity

The shell scopes themes via `[data-nexus-theme="<id>"]` on the root
element. Don't use selectors that bypass this:

```css
/* ❌ no — leaks into other themes */
.editor { background: black; }

/* ✓ yes — scoped to your theme */
:root[data-nexus-theme="midnight"] .editor { background: black; }
```

For component-level overrides, prefer setting CSS variables to
restyling components directly. The variable system is the contract;
restyling components is fragile (selectors break on shell updates).

## Light + dark in one theme

If you want one theme to flip between light and dark with the OS:

```json
"themes": [
  { "id": "midnight-light", "label": "Midnight Light", "type": "light", "css": "light.css" },
  { "id": "midnight-dark",  "label": "Midnight Dark",  "type": "dark",  "css": "dark.css" }
]
```

Two registrations; the user can pin either or pick "Auto" and let the
OS decide.

## Custom fonts

Bundle the font in the plugin and `@font-face` it:

```css
@font-face {
  font-family: 'Berkeley Mono';
  src: url('./fonts/BerkeleyMono.woff2') format('woff2');
}

:root[data-nexus-theme="midnight"] {
  --font-family-mono: 'Berkeley Mono', monospace;
}
```

The shell's CSS resolver loads font URLs relative to the theme's
plugin directory. Keep fonts under 200 KB total — themes are
hot-reloaded and re-downloading large fonts hurts iteration.

## Platform chrome

macOS vibrancy and Windows Mica are stubbed but not yet rendered
natively (see help docs). The CSS variables for window chrome
(`--color-titlebar-*`) work today; native rendering is on the
roadmap.

## Validation

Bad CSS values fall through gracefully — the variable reverts to its
default. Misspelled variable names are silently ignored. Use
**Settings → Appearance → Theme inspector** (planned) to see what's
defined vs. defaulted.

In the meantime, browser devtools work — Inspect → Elements →
Computed styles — to see the resolved value of any `--color-*`.

## Publishing

A theme plugin distributes the same way as any other community
plugin. See [Publishing](../plugins/publishing.md). A clean theme
needs no capabilities, so the install dialog is uneventful.

## See also

- [CSS variable reference](css-variables.md) — every variable, by tier.
- [`../../help/customize/themes.md`](../../help/customize/themes.md)
  — user-facing theme overview.
