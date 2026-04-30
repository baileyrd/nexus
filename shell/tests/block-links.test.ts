/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the BL-049 block-link parser tests that live as a sibling of
 * the implementation under `shell/src/plugins/nexus/editor/blockLinks.test.ts`.
 */
import '../src/plugins/nexus/editor/blockLinks.test.ts'
