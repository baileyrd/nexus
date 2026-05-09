// BL-068 Phase 4 — pure helpers for the Theme Builder's split-view
// preview pane.
//
// The preview renders a representative forge document inside a
// scoped div. CSS-variable inheritance does the heavy lifting: by
// applying a per-mode `--nx-*` override map as `style={...}` on the
// wrapper, the preview reflects what the user is editing in that
// mode — even when the live shell is rendered against the *other*
// mode (the kernel-mode toggle in the picker header).

/** Subset of token keys the preview pane scopes per-mode. Mirrors
 *  the keys edited by `BUILDER_GROUPS` in `ThemeBuilder.tsx` plus a
 *  few derived tokens the representative document references. The
 *  per-mode override is layered over the base-theme value so the
 *  preview shows the *effective* token, not just the user's deltas. */
export const PREVIEW_SCOPED_KEYS: readonly string[] = [
  // Surfaces
  '--nx-bg-primary',
  '--nx-bg-secondary',
  '--nx-bg-tertiary',
  '--nx-bg-elevated',
  // Accent
  '--nx-color-primary',
  '--nx-color-primary-light',
  '--nx-color-primary-dark',
  '--nx-color-secondary',
  // Text
  '--nx-text-primary',
  '--nx-text-secondary',
  '--nx-text-tertiary',
  '--nx-text-muted',
  // Editor / prose
  '--nx-editor-bg',
  '--nx-editor-font-family',
  '--nx-prose-heading-color',
  '--nx-prose-link-color',
  '--nx-callout-bg',
  '--nx-callout-border',
  // Syntax
  '--nx-syntax-keyword',
  '--nx-syntax-string',
  '--nx-syntax-comment',
  '--nx-syntax-function',
  '--nx-syntax-number',
  '--nx-syntax-type',
] as const

/** Pixel width the picker modal should render at. Pure function so
 *  the sizing rule can be unit-tested without rendering React.
 *
 *  - 1300 on the Build tab when both dual mode and the preview pane
 *    are on (two preview columns + the dual editor)
 *  - 1100 on the Build tab with preview but no dual mode (one preview
 *    column + the single editor)
 *  - 960 on the Build tab with dual mode but no preview (current
 *    pre-Phase-4 behaviour)
 *  - 660 everywhere else (Themes / Snippets tabs, Build tab compact) */
export function builderModalWidth(
  activeTab: 'themes' | 'snippets' | 'build',
  dualMode: boolean,
  showPreview: boolean,
): number {
  if (activeTab !== 'build') return 660
  if (dualMode && showPreview) return 1300
  if (showPreview) return 1100
  if (dualMode) return 960
  return 660
}

/** Compose the inline-style CSS-variable map for a single preview
 *  pane. Layers the per-mode `overrides` over the resolved
 *  `baseVars`, restricted to `PREVIEW_SCOPED_KEYS` so the inline-
 *  style payload stays predictable and small.
 *
 *  - `baseVars` is the shape returned by the existing
 *    `extract_resolved_vars` IPC — a flat `Record<varName, value>`
 *    with the active theme's resolved tokens for one of the modes.
 *  - `overrides` is whichever per-mode override map the caller wants
 *    to apply on top: `builderLightOverrides`, `builderDarkOverrides`,
 *    or the single-mode `builderOverrides`.
 *
 *  Returns a `Record<string, string>` ready to spread into a React
 *  `style` prop. Keys missing from both `baseVars` and `overrides`
 *  are omitted so the preview inherits the live shell's value (the
 *  globally-pushed override layer) for any token the builder doesn't
 *  scope per-mode. */
export function composeOverridesForPreview(
  baseVars: Record<string, string>,
  overrides: Record<string, string>,
): Record<string, string> {
  const out: Record<string, string> = {}
  for (const key of PREVIEW_SCOPED_KEYS) {
    const override = overrides[key]
    if (typeof override === 'string' && override.length > 0) {
      out[key] = override
      continue
    }
    const base = baseVars[key]
    if (typeof base === 'string' && base.length > 0) {
      out[key] = base
    }
  }
  return out
}
