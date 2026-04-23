/**
 * WI-11 closer — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the canvas patch-queue unit tests
 * that live as a sibling of the implementation under
 * `shell/src/plugins/nexus/canvas/patchQueue.test.ts`.
 *
 * Same pattern as `tests/editor-store.test.ts`: the sibling file is
 * the canonical location (matches the AI / outline / backlinks
 * convention). This wrapper exists purely so CI runs them
 * automatically — node:test discovers `test()` calls inside any
 * imported module, so the assertions in the imported file are
 * registered as subtests of this file.
 */
import '../src/plugins/nexus/canvas/patchQueue.test.ts'
