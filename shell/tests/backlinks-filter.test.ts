/**
 * Re-export wrapper so the default `pnpm test` glob picks up the
 * BL-049 phase-4 backlinks block-filter store tests living
 * alongside the implementation under
 * `shell/src/plugins/nexus/backlinks/backlinksFilter.test.ts`.
 */
import '../src/plugins/nexus/backlinks/backlinksFilter.test.ts'
