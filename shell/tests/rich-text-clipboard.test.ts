/**
 * C68 (#421) — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the "Copy as rich text" clipboard tests
 * that live as a sibling of the implementation under
 * `shell/src/plugins/nexus/editor/richTextClipboard.test.ts`.
 *
 * Same shim pattern as `tests/tab-context-menu.test.ts`. This file was
 * missing from the original #421 PR — the colocated test ran fine in
 * isolation, but without this wrapper it was silently excluded from
 * `pnpm test`'s default glob.
 */
import '../src/plugins/nexus/editor/richTextClipboard.test.ts'
