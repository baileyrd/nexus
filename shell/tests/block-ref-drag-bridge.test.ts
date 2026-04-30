/**
 * Re-export wrapper so the default `pnpm test` glob picks up the
 * BL-048 phase-3 drag-bridge factory tests living alongside the
 * implementation under
 * `shell/src/plugins/nexus/editor/blockRefDragBridge.test.ts`.
 */
import '../src/plugins/nexus/editor/blockRefDragBridge.test.ts'
