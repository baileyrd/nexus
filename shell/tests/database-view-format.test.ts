/**
 * Re-export wrapper so `pnpm test`'s `tests/*.test.ts` glob picks up
 * the BL-069 type-aware cell-formatter tests.
 */
import '../src/plugins/nexus/editor/cm/databaseViewFormat.test.ts'
