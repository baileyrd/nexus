/**
 * WI-03 closing — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the editor store unit tests that live
 * as a sibling of the implementation under
 * `shell/src/plugins/nexus/editor/editorStore.test.ts`.
 *
 * The sibling file is the canonical location (matches the AI / outline
 * / backlinks pattern). This wrapper exists purely so CI runs them
 * automatically — node:test discovers `test()` calls inside any
 * imported module, so the assertions in the imported file are
 * registered as subtests of this file.
 */
import '../src/plugins/nexus/editor/editorStore.test.ts'
