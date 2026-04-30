/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the BL-049 phase-2 click-navigation tests that live as a
 * sibling of the implementation.
 */
import '../src/plugins/nexus/editor/cm/blockLinkNav.test.ts'
