/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the BL-070 Vim keymap tests that live as a sibling of the
 * implementation.
 */
import '../src/plugins/nexus/editor/cm/vimKeymap.test.ts'
