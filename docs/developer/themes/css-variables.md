# CSS variable reference

Nexus has roughly **497 CSS variables** organized into 10 tiers. This
page describes the **naming conventions and what each tier covers**;
for the authoritative full list, see the source files (linked below).

## Where the canonical list lives

The variable definitions are in the shell's design-system layer:

- `shell/src/styles/tokens/` — one file per tier
- `shell/src/styles/themes/nexus-light.css` — defaults for every
  variable (light)
- `shell/src/styles/themes/nexus-dark.css` — defaults (dark)

For a flat dump of every variable currently defined, run:

```bash
grep -h '^\s*--' shell/src/styles/themes/nexus-light.css | sort -u
```

(A generated machine-readable index is planned; see the gap noted in
[Build a theme](build-a-theme.md).)

## Naming convention

```
--<tier>-<role>-<modifier>?
```

| Tier | Prefix | Examples |
|---|---|---|
| Color | `--color-` | `--color-background-base`, `--color-text-primary` |
| Surface | `--surface-` | `--surface-elevation-1` (drop shadows + bg) |
| Text | `--color-text-` | `--color-text-primary`, `--color-text-link` |
| Border | `--color-border-` | `--color-border-default`, `--color-border-focus` |
| Accent | `--color-accent` | `--color-accent`, `--color-accent-hover` |
| Interactive | `--color-interactive-` | `--color-interactive-hover`, `--color-interactive-pressed` |
| Semantic | `--color-{success,warning,danger,info}-*` | `--color-warning-bg`, `--color-danger-text` |
| Shadow | `--shadow-` | `--shadow-sm`, `--shadow-md`, `--shadow-lg` |
| Motion | `--motion-` | `--motion-duration-fast`, `--motion-easing-ease-out` |
| Typography | `--font-`, `--text-`, `--leading-` | `--font-family-sans`, `--text-base`, `--leading-relaxed` |

Within each tier, modifiers cascade:

- `base` / `default` — the canonical value
- `subtle` / `muted` — softer variant
- `strong` / `prominent` — bolder variant
- `hover` / `pressed` / `disabled` / `focus` — interaction states
- `bg` / `text` / `border` — for semantic colors that span surfaces

## The 10 tiers

### 1. Color (raw palette)

The base palette: hex / oklch values that everything else references.
You almost never read these directly; they exist so tier 2+ can
recombine them.

```css
--color-gray-50 .. --color-gray-950
--color-blue-50 .. --color-blue-950
--color-red-50  .. --color-red-950
--color-green-50 .. --color-green-950
--color-yellow-50 .. --color-yellow-950
```

### 2. Surface

Background colors for stacked surfaces:

```css
--color-background-base       /* the page */
--color-background-surface    /* cards, panels */
--color-background-elevated   /* modals, popovers */
--color-background-overlay    /* command palette backdrop */
```

### 3. Text

Foreground text colors:

```css
--color-text-primary
--color-text-secondary
--color-text-disabled
--color-text-link
--color-text-link-hover
--color-text-on-accent        /* text rendered on accent backgrounds */
```

### 4. Border

Border + divider colors:

```css
--color-border-default
--color-border-muted
--color-border-strong
--color-border-focus           /* focus ring */
```

### 5. Accent

Brand / primary action colors:

```css
--color-accent
--color-accent-hover
--color-accent-pressed
--color-accent-disabled
--color-accent-bg              /* tinted background of the accent */
```

### 6. Interactive

States for clickable / focusable elements:

```css
--color-interactive-hover      /* generic hover background */
--color-interactive-pressed
--color-interactive-selected   /* current row, current tab */
--color-interactive-focus
```

### 7. Semantic

Success / warning / danger / info colors with `bg`, `text`, `border`
variants:

```css
--color-success-bg, --color-success-text, --color-success-border
--color-warning-bg, --color-warning-text, --color-warning-border
--color-danger-bg,  --color-danger-text,  --color-danger-border
--color-info-bg,    --color-info-text,    --color-info-border
```

### 8. Shadow

Drop shadows by elevation:

```css
--shadow-sm, --shadow-md, --shadow-lg, --shadow-xl
--shadow-focus                 /* focus-ring shadow */
```

### 9. Motion

Animation durations and easings:

```css
--motion-duration-fast    /* 100ms */
--motion-duration-base    /* 200ms */
--motion-duration-slow    /* 400ms */
--motion-easing-ease-out
--motion-easing-spring
```

### 10. Typography

Fonts, sizes, weights, line-heights:

```css
--font-family-sans, --font-family-serif, --font-family-mono
--text-xs, --text-sm, --text-base, --text-lg, --text-xl, --text-2xl ...
--font-weight-normal, --font-weight-medium, --font-weight-bold
--leading-tight, --leading-normal, --leading-relaxed
```

## Component-specific variables

Beyond the 10 tiers, individual components publish their own
variables for fine-grained themes:

```css
--editor-font-family, --editor-font-size, --editor-line-height
--terminal-font-family, --terminal-cursor-color
--callout-note-bg, --callout-warning-bg, ...
--titlebar-bg, --titlebar-fg
--statusbar-bg, --statusbar-fg
--scrollbar-thumb-color
```

These default off the tier values — overriding `--color-accent` will
typically pull every component-specific accent with it. Override
component variables only when you want a deliberate exception.

## Defaults and fallbacks

Every variable has a default in `nexus-light.css` / `nexus-dark.css`.
A theme that defines only some variables inherits the rest from the
active base (`type: "light"` or `type: "dark"` in the theme manifest
selects which defaults).

If a variable is undefined everywhere, CSS falls through to the
property default — usually fine, occasionally surprising. Test in
both light and dark by switching base themes.

## Browser-devtools workflow

Open the shell, hit your theme's UI element, inspect:

1. **Computed** tab shows the resolved value.
2. **Styles** tab shows the rule that won.
3. Override a variable inline to preview before editing the file.
4. Copy back to `theme.css` when satisfied.

## A planned generator

A `nexus theme tokens` command (planned) will dump every variable
with its default value as JSON or CSS, so you can diff your theme
against the design system. Until it ships, the `grep` recipe at the
top of this page is the workaround.

## See also

- [Build a theme](build-a-theme.md)
- Live source: `shell/src/styles/themes/`
- [`../../help/customize/themes.md`](../../help/customize/themes.md)
