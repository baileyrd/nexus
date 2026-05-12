# Themes

The shell ships a CSS-variable design system: 497 design tokens
arranged into 10 tiers (color, surface, text, border, accent,
interactive, semantic, shadow, motion, typography). Every visible
element reads from those variables, so a theme is just a set of
overrides.

## Switch themes

In the shell: **Settings → Appearance → Theme**. The list shows every
installed theme. Switching is instant — themes hot-reload over the
event bus without a restart.

## Built-in themes

- **Nexus Light** (default light)
- **Nexus Dark** (default dark)
- **High Contrast Light**
- **High Contrast Dark**

The shell auto-switches between light and dark based on the OS
preference unless you've pinned a specific theme.

## Install a theme

Themes are plugins. Install one like any other community plugin:

```bash
nexus plugin install ./awesome-theme.wasm
```

Or from the **Plugins** panel.

## Author a theme

Scaffold a theme plugin:

```bash
nexus plugin scaffold --template theme --id com.you.cool-theme --name "Cool Theme"
```

You get a `theme.css` and a `plugin.json`. The CSS sets variables:

```css
:root[data-nexus-theme="cool-theme"] {
  --color-background: #1a1b26;
  --color-text: #c0caf5;
  --color-accent: #7aa2f7;
  /* ... 494 more variables ... */
}
```

A reference list of every variable is in
[`docs/developer/themes/css-variables.md`](../../developer/themes/css-variables.md).
Inherit from a base theme to override
only the deltas:

```css
@import url('nexus-theme://nexus-dark');

:root[data-nexus-theme="my-theme"] {
  --color-accent: #ff79c6;
}
```

## Hot reload

Save the CSS and the shell re-applies in under a frame. No restart, no
re-open. Useful for iterating on a theme.

## Platform chrome

macOS vibrancy and Windows Mica are stubbed in CSS but not yet wired
to the native window APIs. They render as solid backgrounds today;
native rendering is on the roadmap.

## CSS snippets (per-forge)

Drop a `.css` file in `<forge>/.forge/snippets/` and toggle it on in
**Settings → Appearance → CSS Snippets**. Snippets layer on top of the
active theme. Useful for one-off tweaks you don't want to package as a
full theme.
