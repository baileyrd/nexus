/**
 * BL-032 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the Cmd+I overlay unit tests that live
 * as siblings of the implementation under
 * `shell/src/plugins/nexus/ai/`.
 *
 * Mirrors `tests/ai-store.test.ts` — node:test discovers `test()`
 * calls inside imported modules, so the assertions in the imported
 * files register as subtests of this file.
 */
import '../src/plugins/nexus/ai/contextContributors.test.ts'
import '../src/plugins/nexus/ai/cmdIStore.test.ts'
import '../src/plugins/nexus/ai/cmdIRuntime.test.ts'
