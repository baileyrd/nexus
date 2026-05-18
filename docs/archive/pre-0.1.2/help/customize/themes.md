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

There is no dedicated `theme` scaffold today — `nexus plugin scaffold`
ships only the `script`, `core`, and `community` templates. Author a
theme by dropping a CSS snippet into your forge:

1. Create `<forge>/.forge/snippets/my-theme.css`.
2. Override the design tokens you care about — every one of the 497
   tokens is overridable. A reference list lives at
   [`docs/developer/themes/css-variables.md`](../../developer/themes/css-variables.md).
3. Toggle the snippet on in **Settings → Appearance → CSS Snippets**.

```css
/* <forge>/.forge/snippets/my-theme.css */
:root[data-nexus-theme="nexus-dark"] {
  --color-background: #1a1b26;
  --color-text: #c0caf5;
  --color-accent: #7aa2f7;
  /* override as many or as few tokens as you like */
}
```

Snippets layer on top of the active theme, so attaching your overrides
to (e.g.) `data-nexus-theme="nexus-dark"` lets your theme follow that
base unless you switch away. Save the file and the shell re-applies
in under a frame — see [Hot reload](#hot-reload) below.

Packaging the snippet as a redistributable plugin (so other users can
install it from a marketplace) is the same path as any other community
plugin and is the eventual home for first-class themes — see WI-44
(marketplace) in the formal-release backlog. For now, snippets are the
authoring path.

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
