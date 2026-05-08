/**
 * Re-export wrapper so `pnpm test`'s `tests/*.test.ts` glob picks up
 * the BL-053 Phase 4 status tests.
 */
import '../src/plugins/nexus/status/StatusPill.test.ts'
import '../src/plugins/nexus/status/statusStore.test.ts'
