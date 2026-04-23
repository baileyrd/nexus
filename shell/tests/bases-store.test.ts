/**
 * WI-10 closing — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the bases-plugin unit tests that live
 * as a sibling of the implementation under
 * `shell/src/plugins/nexus/bases/basesStore.test.ts`.
 *
 * Mirrors the editor / ai / outline shim pattern. node:test discovers
 * `test()` calls in any imported module, so the assertions in the
 * imported file register as subtests of this file.
 */
import '../src/plugins/nexus/bases/basesStore.test.ts'
