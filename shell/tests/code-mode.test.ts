/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the BL-075 dual-mode-router tests that live as a sibling
 * of the implementation under
 * `shell/src/plugins/nexus/editor/codeMode.test.ts`.
 */
import '../src/plugins/nexus/editor/codeMode.test.ts'
