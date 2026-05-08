/**
 * BL-052 follow-up — re-export wrapper so the default `pnpm test`
 * glob picks up the catalog migration unit tests that live as a
 * sibling of the implementation.
 */
import '../src/plugins/catalog.test.ts'
