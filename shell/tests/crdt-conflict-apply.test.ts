/**
 * Re-export wrapper so `pnpm test`'s `tests/*.test.ts` glob picks
 * up the BL-074 apply-resolution helper tests.
 */
import '../src/plugins/nexus/crdtConflict/applyResolution.test.ts'
