> **Archived 2026-04-26** — Three-bug fix plan for the keybindings settings tab. Fixes shipped.

# Plan: Keybindings UI Fixes (Bug 1 · Bug 2 · UX)

## Summary

Three independent issues in the keybindings settings tab of `SettingsPanelView.tsx` and `shell.css`. All fixes are self-contained and can be applied in any order.

---

## Issue 1 — Left nav rail doesn't theme in light mode

### Root Cause

`shell/src/shell/shell.css` line 221:

```css
background: var(--panel-bg-alt, #1f1f20);
```

`--panel-bg-alt` is **never defined** anywhere — not in `index.html`'s `:root` block, not in `[data-theme="light"]`, nowhere in `shell.css`. Because the variable is undefined the fallback `#1f1f20` (near-black) always wins, locking the rail to dark regardless of the active theme.

Every other token used by the rail (`--border`, `--shell-fg`, `--fg-muted`, `--list-hover`, `--list-active`) is properly aliased in `index.html` and responds to `[data-theme="light"]`. Only `--panel-bg-alt` is missing. In contrast, `.settings-panel` uses `var(--panel-bg)`, which is aliased to `var(--background-secondary)` and themes correctly — that's why the main content area switches but the nav doesn't.

### Files and Lines

| File | Location | Change |
|---|---|---|
| `shell/index.html` | `:root` alias block (~line 143) | Add `--panel-bg-alt` alias |

### Implementation

In `shell/index.html`, in the `:root` alias block alongside `--panel-bg`, add:

```css
--panel-bg-alt: var(--background-secondary-alt);
```

`--background-secondary-alt` is defined in both dark (line 31) and light (line 159) theme blocks and resolves correctly for both modes. No change to `shell.css` is needed — the existing `var(--panel-bg-alt, #1f1f20)` will now resolve through the alias.

---

## Issue 2 — "Current" column cells invisible in dark mode

### Root Cause

`SettingsPanelView.tsx` lines 1087–1088 (the `<code>` chip in each keybinding row):

```tsx
background: row.overridden
  ? 'var(--color-accent-bg, #e7f0ff)'
  : 'var(--color-bg-alt, #f3f3f3)',
```

Both `--color-accent-bg` and `--color-bg-alt` are **undefined tokens**. The fallback values (`#e7f0ff` light-blue, `#f3f3f3` near-white) are light colours that look fine in light mode but produce white-on-dark (invisible) in dark mode where the text colour is `--shell-fg` (light/cream).

### Files and Lines

| File | Lines | Change |
|---|---|---|
| `shell/src/plugins/core/settings/SettingsPanelView.tsx` | 1087–1088 | Replace undefined tokens |

### Implementation

```tsx
// Before:
background: row.overridden
  ? 'var(--color-accent-bg, #e7f0ff)'
  : 'var(--color-bg-alt, #f3f3f3)',

// After:
background: row.overridden
  ? 'var(--interactive-accent-soft, rgba(0,0,0,0.09))'
  : 'var(--background-modifier-hover, rgba(0,0,0,0.05))',
```

`--interactive-accent-soft` and `--background-modifier-hover` are both defined in `:root` and overridden in `[data-theme="light"]` — they produce correct contrast in both modes. Optionally add `color: var(--text-normal)` to the same `<code>` style block to pin text colour explicitly.

---

## Issue 3 (UX) — Collapse Current/Default into a single "Shortcut" column

### Root Cause

When `row.overridden === false`, `row.current === row.default`, so both columns show identical text. There's no explanation of why two identical chords appear, which confuses users. The `BindingRow` type already carries all information needed: `current` (active chord), `default` (manifest default), `overridden` (boolean).

### Files and Lines

| File | Lines | Change |
|---|---|---|
| `shell/src/plugins/core/settings/SettingsPanelView.tsx` | `<thead>` and `<tbody>` of the keybindings table | Restructure columns |

### Design

- One column labelled **"Shortcut"** showing `row.current` (the active chord).
- When overridden: show the default chord below in a smaller muted style (e.g. `← Ctrl+,`) so users see what they changed from.
- When overridden: small filled dot indicator inline with the command name.
- Remove the "Default" column entirely.

### Implementation

**1. Update `<thead>`** — remove `<th>Default</th>`, rename `<th>Current</th>` to `<th>Shortcut</th>`.

**2. Update each `<tbody>` row:**

Remove the entire Default `<td>` block.

Replace the Current `<td>` with:

```tsx
<td style={cellStyle}>
  {editing === row.commandId ? (
    <ChordCaptureInput
      onCommit={chord => void handleCommit(row.commandId, chord)}
      onCancel={() => setEditing(null)}
    />
  ) : (
    <div>
      <code style={{
        background: row.overridden
          ? 'var(--interactive-accent-soft, rgba(0,0,0,0.09))'
          : 'var(--background-modifier-hover, rgba(0,0,0,0.05))',
        padding: '2px 6px',
        borderRadius: 3,
        fontSize: '0.9em',
        fontWeight: row.overridden ? 600 : undefined,
      }}>
        {formatChord(row.current) || '—'}
      </code>
      {row.overridden && (
        <div style={{ marginTop: 3, fontSize: '0.78em', opacity: 0.55 }}>
          {'← '}{formatChord(row.default) || '—'}
        </div>
      )}
    </div>
  )}
</td>
```

**3. Add override dot to the Command cell** — inside the `<div>` showing `row.title`:

```tsx
{row.overridden && (
  <span
    title="Override active"
    style={{
      display: 'inline-block',
      width: 6,
      height: 6,
      borderRadius: '50%',
      background: 'var(--interactive-accent)',
      marginLeft: 5,
      verticalAlign: 'middle',
    }}
  />
)}
```

**4. Column widths** — with three columns instead of four, give Actions a fixed `width: 120px` and let Command and Shortcut share remaining space via `table-layout: auto`.

**5. Filter logic** — no change needed; the filter predicate already uses `r.current` for chord searching, which remains the active chord after the column merge.

---

## Non-Goals

- Drag-to-reorder in the keybindings table.
- Migrating `--color-accent-bg` / `--color-bg-alt` references in other files (only the settings panel is in scope).

---

## Critical Files

- `shell/src/plugins/core/settings/SettingsPanelView.tsx`
- `shell/src/shell/shell.css`
- `shell/index.html`
