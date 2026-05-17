/**
 * BL-143 Phase 2.1 — wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the collab store unit tests that live
 * as a sibling of the implementation. Same pattern as ai-store.test.ts.
 */
import '../src/plugins/nexus/collab/collabStore.test.ts'
