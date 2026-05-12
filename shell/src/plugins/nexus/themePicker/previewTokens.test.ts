// BL-068 Phase 4 — unit tests for the preview-overrides composer.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  builderModalWidth,
  composeOverridesForPreview,
  PREVIEW_SCOPED_KEYS,
} from './builderPreview'

test('composeOverridesForPreview falls back to baseVars for unspecified keys', () => {
  const result = composeOverridesForPreview(
    { '--nx-bg-primary': '#111', '--nx-text-primary': '#eee' },
    {},
  )
  assert.strictEqual(result['--nx-bg-primary'], '#111')
  assert.strictEqual(result['--nx-text-primary'], '#eee')
})

test('composeOverridesForPreview prefers override over baseVars', () => {
  const result = composeOverridesForPreview(
    { '--nx-color-primary': '#0066cc' },
    { '--nx-color-primary': '#ff0066' },
  )
  assert.strictEqual(result['--nx-color-primary'], '#ff0066')
})

test('composeOverridesForPreview drops empty-string overrides and falls back', () => {
  // The builder clears an override by writing an empty string; the
  // preview should fall back to base rather than render `:root` with
  // a literally empty value (which makes the var invalid).
  const result = composeOverridesForPreview(
    { '--nx-bg-primary': '#111' },
    { '--nx-bg-primary': '' },
  )
  assert.strictEqual(result['--nx-bg-primary'], '#111')
})

test('composeOverridesForPreview omits keys outside PREVIEW_SCOPED_KEYS', () => {
  // Out-of-scope tokens (anything not in PREVIEW_SCOPED_KEYS) should
  // not be projected — the preview inherits the live shell's value
  // for those, keeping the inline-style payload bounded.
  const result = composeOverridesForPreview(
    { '--nx-some-other-token': '#abc' },
    { '--nx-some-other-token': '#def' },
  )
  assert.strictEqual('--nx-some-other-token' in result, false)
})

test('composeOverridesForPreview omits a key absent from both inputs', () => {
  const result = composeOverridesForPreview({}, {})
  // The result is empty — every scoped key is missing from both
  // sides so nothing makes it through.
  assert.deepStrictEqual(result, {})
})

test('PREVIEW_SCOPED_KEYS covers every BUILDER_GROUPS variable', () => {
  // Pin the contract: anything the user can edit in ThemeBuilder's
  // BUILDER_GROUPS should also be projectable into the preview.
  // Values copied verbatim from `ThemeBuilder.tsx`'s BUILDER_GROUPS
  // — drift is what this test catches.
  const builderGroupKeys = [
    '--nx-bg-primary', '--nx-bg-secondary', '--nx-bg-tertiary', '--nx-bg-elevated',
    '--nx-color-primary', '--nx-color-primary-light', '--nx-color-primary-dark', '--nx-color-secondary',
    '--nx-text-primary', '--nx-text-secondary', '--nx-text-tertiary', '--nx-text-muted',
    '--nx-editor-bg', '--nx-editor-font-family', '--nx-prose-heading-color', '--nx-prose-link-color',
    '--nx-callout-bg', '--nx-callout-border',
    '--nx-syntax-keyword', '--nx-syntax-string', '--nx-syntax-comment', '--nx-syntax-function',
    '--nx-syntax-number', '--nx-syntax-type',
  ]
  for (const key of builderGroupKeys) {
    assert.ok(
      PREVIEW_SCOPED_KEYS.includes(key),
      `${key} is editable in ThemeBuilder but missing from PREVIEW_SCOPED_KEYS`,
    )
  }
})

test('builderModalWidth: themes tab is 660 regardless of build flags', () => {
  assert.strictEqual(builderModalWidth('themes', false, false), 660)
  assert.strictEqual(builderModalWidth('themes', true, true), 660)
  assert.strictEqual(builderModalWidth('snippets', true, true), 660)
})

test('builderModalWidth: build tab compact is 660', () => {
  assert.strictEqual(builderModalWidth('build', false, false), 660)
})

test('builderModalWidth: build tab dual-only is 960', () => {
  assert.strictEqual(builderModalWidth('build', true, false), 960)
})

test('builderModalWidth: build tab preview-only is 1100', () => {
  assert.strictEqual(builderModalWidth('build', false, true), 1100)
})

test('builderModalWidth: build tab dual + preview is 1300', () => {
  assert.strictEqual(builderModalWidth('build', true, true), 1300)
})

test('composeOverridesForPreview returns a flat string-valued map', () => {
  // The result is spread into `style={...}` — every value must be a
  // string so React applies it as a CSS property. The composer must
  // never propagate an `undefined` even when the override map happens
  // to carry one.
  const result = composeOverridesForPreview(
    { '--nx-bg-primary': '#000' },
    { '--nx-bg-primary': undefined as unknown as string },
  )
  assert.strictEqual(typeof result['--nx-bg-primary'], 'string')
  assert.strictEqual(result['--nx-bg-primary'], '#000')
})
