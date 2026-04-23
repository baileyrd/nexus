/**
 * WI-02 part 2 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the theme store unit tests that live
 * as a sibling of the implementation under
 * `shell/src/stores/themeStore.test.ts`.
 *
 * The sibling file is the canonical location (matches the AI / editor
 * pattern). This wrapper exists purely so CI runs them automatically —
 * node:test discovers `test()` calls inside any imported module, so
 * the assertions in the imported file are registered as subtests of
 * this file.
 */
import '../src/stores/themeStore.test.ts'
