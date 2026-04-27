// Runtime smoke test for the live-preview extension.
//
// Pure-builder tests (livePreviewDecorations.test.ts) cover decoration
// shape but cannot catch CM6 mount-time constraints — see commit 29e637c
// where block decorations from a ViewPlugin throw `RangeError: Block
// decorations may not be specified via plugins`. This file is the place
// to actually mount an `EditorView` and assert no throw on selection
// changes.
//
// TODO: this test is currently skipped because the shell test suite has
// no DOM shim. The runner is plain `node --import tsx --test` (see
// `shell/package.json`'s `test` script) and neither `jsdom` nor
// `happy-dom` is a dependency. Adding one introduces install + runtime
// cost across the whole suite, so we flag the gap here instead and
// leave the runtime cliff covered by manual QA / e2e (`pnpm e2e`).
//
// To revive: install a DOM shim (e.g. `pnpm --filter nexus-shell add -D
// happy-dom`), wire it via a `--import` setup file that calls
// `GlobalRegistrator.register()`, and replace `test.skip` with `test`.

import { test } from 'node:test'

test.skip('livePreview: mounts an EditorView with table/hr/heading widgets without throwing', () => {
  // Intentional placeholder — see TODO at top of file.
})
