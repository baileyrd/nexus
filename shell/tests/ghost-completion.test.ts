/**
 * Re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the BL-034 ghost completion tests
 * that live as a sibling of the implementation under
 * `shell/src/plugins/nexus/editor/cm/ghostCompletion.test.ts`.
 */
import '../src/plugins/nexus/editor/cm/ghostCompletion.test.ts'
