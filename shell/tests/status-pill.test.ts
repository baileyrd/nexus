/**
 * Re-export wrapper so `pnpm test`'s `tests/*.test.ts` glob picks up
 * the BL-053 Phase 4 status tests. The implementation moved under
 * `nexus.files` (status indicators are owned by the file-tree plugin),
 * so the colocated tests live there too.
 */
import '../src/plugins/nexus/files/status/StatusPill.test.ts'
import '../src/plugins/nexus/files/status/statusStore.test.ts'
