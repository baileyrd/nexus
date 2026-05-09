/**
 * Re-export wrapper so `pnpm test`'s `tests/*.test.ts` glob picks
 * up the BL-074 resolver-modal store tests.
 */
import '../src/plugins/nexus/crdtConflict/conflictStore.test.ts'
