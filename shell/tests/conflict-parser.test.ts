/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the BL-084 conflict-marker parser tests that live as a
 * sibling of the implementation.
 */
import '../src/plugins/nexus/gitPanel/conflict/conflictParser.test.ts'
