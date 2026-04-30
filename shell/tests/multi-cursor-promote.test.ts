/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the BL-051 multi-cursor-from-blocks tests that live as a
 * sibling of the implementation.
 */
import '../src/plugins/nexus/editor/cm/multiCursorPromote.test.ts'
