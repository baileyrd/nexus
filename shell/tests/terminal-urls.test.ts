/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the BL-058 URL detection / extractor tests that live as a
 * sibling of the implementation.
 */
import '../src/plugins/nexus/terminal/urls.test.ts'
