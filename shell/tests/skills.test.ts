/**
 * WI-08 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the skills store unit tests that live
 * as a sibling of the implementation under
 * `shell/src/plugins/nexus/skills/skillsStore.test.ts`.
 *
 * Same shim pattern as `saved-commands.test.ts` — node:test discovers
 * `test()` calls inside any imported module, so the assertions in the
 * imported file are registered as subtests of this file.
 */
import '../src/plugins/nexus/skills/skillsStore.test.ts'
