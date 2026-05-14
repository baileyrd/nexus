/**
 * Re-export wrapper so `pnpm test` (glob `tests/*.test.ts`) picks up
 * the src-colocated BL-067 Phase 0 tests at
 * `src/host/layoutSnapshot.test.ts`. See `tests/workspace-ViewRegistry.test.ts`
 * for the same shim pattern.
 */
import '../src/host/layoutSnapshot.test.ts'
