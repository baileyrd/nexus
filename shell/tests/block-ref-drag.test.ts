/**
 * Re-export wrapper so the default `pnpm test` glob picks up the
 * BL-048 block-ref drag-contract tests that live as a sibling of
 * the implementation under
 * `shell/src/plugins/nexus/editor/blockRefDrag.test.ts`.
 */
import '../src/plugins/nexus/editor/blockRefDrag.test.ts'
