/**
 * Re-export wrapper so `pnpm test`'s `tests/*.test.ts` glob picks up
 * the BL-077 / BL-076 follow-up tests (save-format hook registry +
 * reveal-line consumer helper).
 */
import '../src/plugins/nexus/editor/cm/saveFormatHooks.test.ts'
import '../src/plugins/nexus/editor/cm/revealLine.test.ts'
