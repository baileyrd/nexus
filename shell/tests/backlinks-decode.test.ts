/**
 * Re-export wrapper so the default `pnpm test` glob picks up the
 * BL-049 phase-3 backlinks-fragment decoder tests living
 * alongside the implementation under
 * `shell/src/plugins/nexus/backlinks/backlinksDecode.test.ts`.
 */
import '../src/plugins/nexus/backlinks/backlinksDecode.test.ts'
