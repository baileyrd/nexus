# Obsidian UI Chrome — Measurements Reference

Source of truth: `obsidian-unpacked/app.css` (≈20,000 lines) with supporting
notes from `main.js` (Electron main). `app.js` / `starter.js` / `enhance.js`
in the extract are zero-byte stubs — Obsidian downloads the actual UI JS from
its CDN at runtime, so the CSS is the authoritative measurement source.

## How to read this document

- Every dimension is traced back to a concrete source, in the form
  `value (app.css:LINE, --custom-prop-name)`. If a value is produced by a
  `calc()` expression or a cascade of custom-properties, the final computed
  default is shown and the chain is noted inline.
- State classes that change a dimension are listed under each region with
  the exact override rule and selector.
- "Not specified" means the selector has no explicit width/height in app.css
  — it is either flex-sized or computed at runtime by JS and cannot be pinned
  from CSS alone.
- Line numbers are app.css line numbers unless otherwise noted.

Platform modifiers used throughout: `mod-macos`, `mod-windows`, `mod-linux`.
Frame modifiers: `is-frameless` (frameless window), `is-hidden-frameless`
(frameless and titlebar visually collapsed into tab-row), `is-fullscreen`,
`is-maximized`, `is-focused`.

---

## Global tokens

### Dimension tokens (`:root` / `body`)

| Token | Value | Line |
|---|---|---|
| `--header-height` | `40px` | 2237 |
| `--titlebar-height` | `30px` (on `body`) | 3960 |
| `--ribbon-width` | `44px` | 2598 |
| `--divider-width` | `1px` | 2197 |
| `--divider-width-hover` | `3px` | 2198 |
| `--divider-vertical-height` | `calc(100% - var(--header-height))` | 2199 |
| `--tab-outline-width` | `1px` | 2696 |
| `--radius-s` | `4px` | 2582 |
| `--radius-m` | `8px` | 2583 |
| `--size-2-1 / 2-2 / 2-3` | `2px / 4px / 6px` | 2624–2626 |
| `--size-4-1 / 4-2 / 4-3 / 4-4` | `4px / 8px / 12px / 16px` | 2627–2630 |
| `--size-4-6 / 4-8 / 4-9 / 4-10` | `24px / 32px / 36px / 40px` | 2632–2635 |
| `--size-4-12 / 4-16 / 4-18` | `48px / 64px / 72px` | 2636–2638 |
| `--icon-s / -m / -l` | `16px / 18px / 18px` | 2337–2339 |
| `--icon-s-stroke-width` | `2px` | 2342 |
| `--icon-m-stroke / -l-stroke` | `1.75px / 1.75px` | 2343–2344 |
| `--ribbon-padding` | `8px 4px 12px` (from size tokens) | 2599 |
| `--frame-left-space` (macOS) | `calc(80px - var(--ribbon-width))` → `36px` | 3963 |
| `--frame-right-space` (macOS) | `0px` | 3964 |
| `--frame-left-space` (win/linux) | `0px` | 3971 |
| `--frame-right-space` (win/linux) | `126px` | 3972 |
| `--traffic-lights-offset-x` | `var(--header-height)` → `40px` | 2774 |
| `--traffic-lights-offset-y` | `var(--header-height)` → `40px` | 2775 |

### Layer stack (z-index tokens)

| Token | Value | Line |
|---|---|---|
| `--layer-cover` | `5` | 2389 |
| `--layer-sidedock` | `10` | 2390 |
| `--layer-status-bar` | `15` | 2391 |
| `--layer-popover` | `30` | 2392 |
| `--layer-modal` | `50` | 2394 |
| `--layer-menu` | `65` | 2396 |
| `--layer-tooltip` | `70` | 2397 |
| `--layer-dragged-item` | `80` | 2398 |

### Region-level color tokens (chain from base palette)

| Token | Default source | Line |
|---|---|---|
| `--background-primary` | `var(--color-base-00)` | 2824 |
| `--background-secondary` | `var(--color-base-20)` | 2826 |
| `--background-secondary-alt` | `var(--color-base-05)` (light) / `--color-base-30` (dark) | 2903 / 2952 |
| `--text-normal` | `var(--color-base-100)` | 2842 |
| `--text-muted` | `var(--color-base-70)` | 2843 |
| `--divider-color` | `var(--background-modifier-border)` | 2195 |
| `--titlebar-background` | `var(--background-secondary)` | 2776 |
| `--titlebar-background-focused` | `var(--background-secondary-alt)` | 2777 |
| `--titlebar-border-width` | `0px` | 2778 |
| `--titlebar-text-color` | `var(--text-muted)` | 2780 |
| `--titlebar-text-color-focused` | `var(--text-normal)` | 2781 |
| `--tab-container-background` | `var(--background-secondary)` | 2693 |
| `--file-header-background` | `var(--background-primary)` | 2233 |
| `--file-header-background-focused` | `var(--background-primary)` | 2234 |
| `--file-header-border` | `var(--border-width) solid transparent` | 2235 |
| `--ribbon-background` | `var(--background-secondary)` | 2596 |
| `--ribbon-background-collapsed` | `var(--background-primary)` | 2597 |
| `--status-bar-background` | `var(--background-secondary)` | 2658 |
| `--status-bar-text-color` | `var(--text-muted)` | 2662 |
| `--status-bar-border-color` | `var(--divider-color)` | 2659 |
| `--status-bar-border-width` | `var(--border-width) 0 0 var(--border-width)` | 2660 |
| `--status-bar-radius` | `var(--radius-m) 0 0 0` | 2664 |
| `--status-bar-font-size` | `var(--font-ui-smaller)` | 2661 |
| `--status-bar-position` | `fixed` | 2663 |

---

## Window / root

The Electron root window is constructed in `main.js` (minified). Known-good
extracts from that file:

- User-config toggles `frame: true/false` and `titleBarStyle: "default" / "hidden"`.
- With `frame: "native"` (default), the OS chrome owns the top edge and the
  CSS `.titlebar` stays 0-height on Win/Linux.
- When the user enables "hide titlebar", the `body` gets `.is-hidden-frameless`
  and `.mod-windows` / `.mod-linux` / `.mod-macos`, which drive the rules
  documented below.

The HTML skeleton loaded by the window is `index.html` (≈1.4 KB) which only
loads `app.js` (0-byte stub in the extract — real code is CDN-loaded). All
layout is therefore CSS-driven on top of whatever tree `app.js` produces.

---

## Titlebar + window controls + drag regions

### Key selectors

- `.titlebar` (5607–5616): `position: fixed; top:0; left:0; right:0;` with
  `-webkit-app-region: drag` and `background: var(--titlebar-background)`.
- `.titlebar-inner` (5617–5622): fills the titlebar, supplies text color.
- `.titlebar-text` (5626–5644): centered window title; padded `0 125px`
  (reserves space for the traffic lights / window controls).
- `.titlebar-button-container` (5645–5649): absolute-positioned button
  cluster. `top: 8px` on macOS (5650–5652).
- `.titlebar-button-container.mod-left` (5653): `left: 0`. macOS bumps it
  to `left: calc(80px / var(--zoom-factor))` (5656) to clear the traffic lights.
- `.titlebar-button-container.mod-right` (5675): `right: 0` — hosts min/max/close.
- `.titlebar-button` (5678–5684): `padding: var(--size-2-2) var(--size-2-3)`;
  `-webkit-app-region: no-drag` so clicks land.

### Height cascade

| Body class combo | `.titlebar` height | Line |
|---|---|---|
| `is-frameless` and NOT `is-hidden-frameless` NOT `is-fullscreen` | `var(--titlebar-height)` = **30px** | 3979–3982 |
| `is-frameless:not(.is-hidden-frameless):not(.is-fullscreen):not(.is-maximized)` | additionally `padding-top: 2px` (3985) | 3984 |
| `is-frameless.is-hidden-frameless` | `calc(var(--header-height) - 1px)` = **39px** | 3987–3988 |
| `is-frameless.is-hidden-frameless.starter` | `var(--titlebar-height)` = **30px** | 3990–3991 |
| `is-fullscreen` | `display: none` | 3993–3994 |
| `is-hidden-frameless` on macOS | `display: none` (5701–5703) — controls move to tab row | 5701 |
| `is-hidden-frameless.mod-windows / mod-linux` | transparent, pointer-events none; buttons re-enabled via `pointer-events: auto` | 5704–5710 |

### Platform-specific button sizing

- Windows / Linux: `.titlebar-button { padding: 0 16px }` (5728–5731), full-height
  container (5723–5725); close-hover background is `--background-modifier-error`.
- macOS: button padding stays at `--size-2-2 / --size-2-3` and gets
  `border-radius: var(--radius-s)` (5694–5695). Traffic-light cluster sits
  8px below the top edge of the titlebar (5650–5651).

### Drag vs no-drag regions

- Default `.titlebar` is `-webkit-app-region: drag` (5608).
- `.titlebar-button` is `no-drag` (5679) so buttons receive clicks.
- `.is-hidden-frameless:not(.starter) .titlebar` becomes `no-drag` (4022–4024).
- When titlebar is collapsed into the tab row (hidden-frameless), drag regions
  are re-created on the tab container:
  - `.workspace-tab-header-container-inner` is `drag` (6106).
  - The `::before` pseudo on `mod-top-left-space` and `::after` on
    `mod-top-right-space` are `no-drag` and reserve `--frame-left-space` /
    `--frame-right-space` pixels for OS traffic lights / window buttons
    (4043–4059).

### Important helpers

- `.workspace-tabs.mod-top-left-space .workspace-tab-header-container` pads
  `var(--size-4-2) + var(--frame-left-space)` on the start (4037–4038) — reserves
  traffic-light well on macOS.
- `.workspace-tabs.mod-top-right-space .workspace-tab-header-container` pads
  `var(--size-4-2) + var(--frame-right-space)` on the end (4040–4041) — reserves
  126px for min/max/close on Win/Linux.
- `.mod-macos.is-hidden-frameless:not(.is-popout-window) .sidebar-toggle-button.mod-right`
  is `position: fixed; top: 0; right: 0` (6621–6628). macOS quirk: the right
  sidebar-toggle overlays the tab row from the corner.
- `.mod-macos.is-hidden-frameless:not(.is-popout-window) .workspace .workspace-tabs.mod-top-right-space .workspace-tab-header-container`
  gets a `padding-right: 38px` bump (6629–6630) to clear the fixed toggle.

---

## Ribbon (activity bar) — `.workspace-ribbon.mod-left`

### Key selectors

- `.workspace-ribbon` (4416–4428): `width: var(--ribbon-width)` = **44px**,
  `flex: 0 0 44px`, column flex, `background: var(--ribbon-background)`,
  `padding: var(--ribbon-padding)` = **8px 4px 12px**, gap `var(--size-4-1)` = 4px,
  right border `var(--divider-width) solid var(--divider-color)`.
- `.workspace-ribbon.mod-left` (4393–4396): `margin-top: var(--header-height)` = 40px.
  The ribbon starts below the top corner well.
- `.workspace-ribbon.mod-left:before` (4397–4408): fills the 40×44 corner
  above the ribbon with `--titlebar-background` and a bottom tab-outline
  divider; also `-webkit-app-region: drag` so that corner is draggable.
- `.workspace-ribbon.mod-left.is-collapsed` (4429–4433): 250ms fade to
  `--ribbon-background-collapsed` = `--background-primary`.
- `.workspace-ribbon.mod-right` (4434–4436): `display: none` — right ribbon
  is not rendered on desktop; right-side affordances live inside the right
  sidedock's tab-header instead.
- `.workspace-ribbon.is-collapsed` (4440–4442): fallback bg.

### Sidebar toggle button

- `.sidebar-toggle-button` (6591–6601): `height: calc(var(--header-height) - 1px)` = **39px**,
  padding `var(--size-4-2) 0 7px 0`, icon size `--icon-l` = 18px.
- `.workspace-ribbon .sidebar-toggle-button` (4011–4016): `position: absolute;
  top: 0; left: 0; width: var(--ribbon-width)` — sits in the 40×44 corner well.
- `.sidebar-toggle-button.mod-left` icon width is `--sidebar-left-toggle-inner-width`.
- `.sidebar-toggle-button.mod-right` mirrored with `transform: scale(-1, 1)` (6611–6613).

### State modifiers

- `body:not(.show-ribbon)` overrides `--ribbon-width: 0px` (4409–4410) AND
  `display: none` on `.workspace-ribbon, .side-dock-ribbon` (4412–4414).
- `.workspace-ribbon.is-hidden` → `display: none` (4437–4439).
- `.side-dock-settings { margin-top: auto }` (4451–4453) pins the vault-
  profile / settings cluster to the bottom.

---

## Sidebar (left-split) — `.workspace-split.mod-left-split`

### Structure (outer → inner)

1. `.workspace-split.mod-left-split`
2. `.workspace-tabs.mod-top` (tab row with inspector tabs)
3. `.workspace-tab-header-container` (36px-ish header row; see measurements)
4. `.workspace-leaf` → `.workspace-leaf-content` → `.view-header` (HIDDEN here)
5. `.view-content` → per-view body, e.g. files view:
   - `.nav-header` → `.search-input-container` + `.nav-buttons-container`
   - `.nav-files-container` (the tree)

### Key selectors & dimensions

- `.workspace-split.mod-left-split, .mod-right-split` (5957–5960):
  `flex: 0 0 auto` — fixed width set by JS (resizable by user).
- `.workspace-split.mod-left-split .view-header, .mod-right-split .view-header`
  (4204–4207): `display: none` — view-headers are not rendered in side panels.
- `.workspace-split.mod-left-split .view-content, .mod-right-split .view-content`
  (4298–4301): `height: 100%; overflow: auto` (overrides the `calc(100% - --header-height)`
  rule that applies in the main editor area).
- `.workspace-tab-header-container` (6092–6101):
  - `height: var(--header-height)` = **40px**
  - background `var(--tab-container-background)`
  - border-bottom `var(--tab-outline-width) solid var(--tab-outline-color)`
  - padding `0 var(--size-4-2)` = 0 8px
- `.workspace-tab-header-container-inner` (6105–6115): `-webkit-app-region: drag`,
  margin `6px -5px calc(var(--tab-outline-width) * -1)`, padding `1px 15px 0`.
- `.mod-left-split .workspace-tab-header-container .workspace-tab-header-container-inner`,
  same for `.mod-right-split` (6392–6393): overrides specific to side panels.
- `.nav-header` (8344–8346): `padding: var(--size-4-2)` = **8px** all round.
- `.nav-header .search-input-container` (8347–8350): `margin: 4px auto; padding: 0;`.
- `.nav-buttons-container` (8351–8354): `flex-wrap: wrap; gap: var(--size-2-1)` = 2px.
- `.nav-buttons-container.has-separator` (8355–8359): adds 1px bottom border +
  `padding-bottom: var(--size-2-3)` + `margin-bottom: var(--size-4-2)`.
- `.workspace-sidedock-vault-profile` (5778–5830, desktop only): the bottom
  vault-switcher strip. Internal icon and name typography pinned by selector.

### Resize handle

- `.workspace-split.mod-left-split > .workspace-leaf-resize-handle` (5926–5941):
  `width: var(--divider-width-hover)` = **3px**, `height: var(--divider-vertical-height)`,
  cursor `col-resize`, border-inline-end `var(--divider-width) solid var(--divider-color)`.
- `.workspace-split.mod-left-split > .workspace-leaf-resize-handle`
  (5965–5971): `z-index: var(--layer-status-bar)` = 15, bottom: 0.

### Width

Not specified in app.css — set dynamically by JS and persisted. The split has
only `flex: 0 0 auto` and no min/max width rules at the split level.

---

## Editor / root split — `.workspace-split.mod-root`

### Stack

1. `.workspace-split.mod-root { background: var(--background-primary); }` (5861–5863)
2. `.workspace-tabs.mod-top` (if tabs visible) → `.workspace-tab-header-container`
3. `.workspace-leaf`
4. `.view-header` — **visible here** because the `display: none` rule at
   4198–4200 requires `:not(.show-view-header):not(.is-phone)`, and the blanket
   hide at 4204–4207 excludes `.mod-root`.
5. `.view-content`

### Key selectors

- `.workspace-tab-header-container` — same 40px as in sidebars but with
  root-specific inner padding at 6113–6115 (`padding: 1px 15px 0`).
- `.view-header` (4188–4196):
  - `height: var(--header-height)` = **40px**
  - `display: flex; gap: var(--size-4-2) = 8px; padding: 0 var(--size-4-3) = 0 12px`
  - `background: var(--file-header-background)` = `--background-primary`
  - `border-bottom: var(--file-header-border)` = `var(--border-width) solid transparent`
- `.is-focused .workspace-leaf.mod-active .view-header` (4201–4203):
  background flips to `--file-header-background-focused`.
- `.view-header-left` (4218–4222): flex, start-aligned — hosts nav buttons.
- `.view-header-title-container` (4223–4235): flex 1, typography from
  `--file-header-font / -size / -weight` (2230–2232), centered by
  `justify-content: var(--file-header-justify)` = `center` (2236).
- `.view-header-nav-buttons` (5972–5976): `--icon-size: var(--icon-s)` = 16px.
  Hidden on phone (5977–5979).
- `.view-content` (4288–4291): `width: 100%; height: calc(100% - var(--header-height))`
  = below the 40px view-header.
- `.workspace-split.mod-root .view-content` (4292–4294): background
  `--background-primary`.
- `.workspace-tab-header-container .workspace-tab-header.is-active` (6189)
  and `::before/::after` pseudos at 6194–6198 draw the active-tab outline
  using `--tab-outline-width` = 1px.

### Tabs stacked mode

- `.workspace .mod-root .workspace-tabs.mod-stacked .workspace-tab-container .workspace-leaf`
  (6582–6587): `width: var(--tab-stacked-pane-width)` (not declared at root;
  assumed runtime-set). `--tab-stacked-header-width: var(--header-height)` = 40px (2711).

---

## Right panel (right-split) — `.workspace-split.mod-right-split`

### Structure

Same tabbed skeleton as the left split, but with two extra quirks:

1. `.workspace-tabs.mod-top-right-space` — the tab row in the rightmost split
   is flagged so the right edge reserves `--frame-right-space` (126px on
   Win/Linux, 0 on macOS) — see rule at 4040–4041.
2. `.titlebar-button-container.mod-right` is rendered **inside** the rightmost
   `.workspace-tab-header-container`, not in the `.titlebar` element, on
   Win/Linux in hidden-frameless mode. This is why min/max/close appear
   above the inspector tab row.

### Key selectors

- `.workspace-split.mod-right-split .workspace-tabs` (6076–6078):
  `padding-inline-end: 0`.
- `.workspace-split.mod-right-split > .workspace-leaf-resize-handle`
  (5934–5945): `border-inline-start-width: var(--divider-width)`;
  `inset-inline-start: 0` (grows leftward from the split edge).
- `.workspace-split.mod-right-split .view-header` — **hidden**, as with the
  left split (4204–4207).
- `.sidebar-toggle-button.mod-right` (6611–6619): mirrored via
  `transform: scale(-1, 1)`; width token `--sidebar-right-toggle-inner-width`
  (opens to `-open` variant when `.workspace.is-right-sidedock-open`).
- `.mod-macos.is-hidden-frameless:not(.is-popout-window) .sidebar-toggle-button.mod-right`
  (6621–6628): **macOS quirk** — `position: fixed; top: 0; right: 0;` with
  `background: var(--tab-container-background)` and `z-index: var(--layer-cover)` = 5.
  The toggle floats in the top-right corner regardless of the right split's
  collapsed / open state.
- `.is-hidden-frameless:not(.is-fullscreen) .titlebar-button-container.mod-right`
  (4061–4065): filled with `--titlebar-background` (or `-focused` if focused).

### Width

Not specified in app.css — set dynamically by JS, persisted by the workspace.

---

## Status bar — `.status-bar`

### Key selectors

- `.status-bar` (5536–5556):
  - `position: var(--status-bar-position)` = **fixed** (2663)
  - `bottom: 0; right: 0; width: auto` — only the items dictate width
  - `min-height: 18px`
  - `padding: var(--size-4-1)` = **4px**
  - `gap: var(--size-4-1)` = **4px**
  - `font-size: var(--status-bar-font-size)` = `var(--font-ui-smaller)`
  - `border-radius: var(--status-bar-radius)` = `var(--radius-m) 0 0 0`
    (**8px 0 0 0** — rounded upper-left corner only)
  - `border-width: var(--status-bar-border-width)` = `var(--border-width) 0 0 var(--border-width)`
    (top + left only)
  - `z-index: var(--layer-status-bar)` = 15
- `body:not(.is-fullscreen) .status-bar` (5557–5559): `padding-right: var(--size-4-2)`
  = 8px, to avoid crowding the right screen edge.
- `.status-bar-item` (5560–5567): `padding: 3px var(--size-2-2)` = 3px 4px,
  `border-radius: var(--radius-s)` = 4px, `line-height: 1`.
- `.status-bar-item-segment` (5595–5599): `margin-inline-end: var(--size-4-2)`
  = 8px, gap 4px.
- `.is-screenshotting .status-bar` (5604–5606): `display: none`.

Since the status bar is `position: fixed; bottom: 0; right: 0`, it floats
above the right split's content and does not take flex space from the
workspace. On mobile (`.is-mobile .status-bar`, 18802) it is repositioned
into flow; desktop behavior is the fixed-corner badge.

---

## State modifiers

### Frame state

| Class | Effect (selected rules) |
|---|---|
| `is-frameless` | Adds padding-top to body; shows `.titlebar` at 30px (3974–3976, 3979–3982). Pairs with `in-progress` during startup. |
| `is-hidden-frameless` | Titlebar collapses to `calc(--header-height - 1px)` = 39px (3987–3988); titlebar becomes `no-drag` (4022–4024) UNLESS `.starter`; on macOS `display: none`; on Win/Linux transparent, no pointer events except buttons (5701–5721). Also sets `--divider-vertical-height: 100%` (4008–4010). |
| `is-hidden-frameless.starter` | Titlebar restored to 30px (3990–3991); also keeps the `drag` region (see 4022). |
| `.is-hidden-frameless .titlebar-button.mod-logo` | Logo button hidden (4025–4027). |
| `.in-progress` | Makes titlebar bg `--background-primary` for boot screen (4028–4035); forces `-webkit-app-region: drag` and z-index 10001. |

### Platform

| Class | Effect |
|---|---|
| `mod-macos` | Sets `--frame-left-space: calc(80px - --ribbon-width)` = 36px (3963). Titlebar-button-container `top: 8px` (5650). Left container shifted by 80px for traffic lights (5656). Buttons get `--radius-s` (5694). macOS-only right-toggle fixed quirk (6621). |
| `mod-windows`, `mod-linux` | Both set `--frame-left-space: 0`, `--frame-right-space: 126px` (3971–3972). Titlebar-button-container full-height (5723–5725). Button padding `0 16px` (5728–5731). Close hover: error bg + white icon (5738–5746). |

### Window state

| Class | Effect |
|---|---|
| `is-maximized` | Removes the `padding-top: 2px` on frameless titlebar (3983–3985). |
| `is-fullscreen` | Hides `.titlebar` entirely (3993–3994). Removes status-bar right padding (5557–5559 reverse). Disables the two left/right tab-spacer padding rules (4037, 4040 require `:not(.is-fullscreen)`). |
| `is-focused` | Flips `--titlebar-background` to `-focused` variant and `--tab-container-background` on `mod-top` (4000–4006). Also swaps view-header, tab-header, and titlebar text colors (6132–6147, 4201–4203, 5623–5625). |

### Visibility

| Class | Effect |
|---|---|
| `show-view-header` | Whitelist: `.view-header` visible in all panes, not just `mod-root`. Without it, `.view-header` hidden for non-root / non-phone (4198–4200). |
| `show-ribbon` | Without it (`body:not(.show-ribbon)`), `--ribbon-width: 0px` AND `.workspace-ribbon` `display: none` (4409–4414). |
| `is-translucent` | Hides tab-header bg on collapsed side splits (5961–5964) and adjusts sidebar-toggle chrome (5765–5768). Acrylic / blur mode. |
| `starter` | Keeps 30px titlebar even in hidden-frameless (3990–3991); special titlebar rules at 17482–17493. |

---

## Source files summary

- `app.css` — 600 KB, all chrome CSS rules. Primary reference.
- `main.js` — 60 KB, minified Electron main. `BrowserWindow` config, menu
  accelerators, zoom keybindings.
- `index.html` / `starter.html` — 1.4 KB shells that load `app.js` and the
  boot CSS.
- `app.js` / `starter.js` / `enhance.js` — 0-byte stubs in the extract; real
  UI code is fetched from Obsidian's CDN at runtime. Do not read.
