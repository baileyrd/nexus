/**
 * WI-01 Slice A — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the AI store unit tests that live as
 * a sibling of the implementation under
 * `shell/src/plugins/nexus/ai/aiStore.test.ts`.
 *
 * The sibling file is the canonical location (matches the editor /
 * outline / backlinks pattern). This wrapper exists purely so CI runs
 * them automatically — node:test discovers `test()` calls inside any
 * imported module, so the assertions in the imported file are
 * registered as subtests of this file.
 */
import '../src/plugins/nexus/ai/aiStore.test.ts'
