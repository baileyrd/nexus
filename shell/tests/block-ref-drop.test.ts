/**
 * Re-export wrapper so the default `pnpm test` glob picks up the
 * BL-048 canvas-drop tests that live alongside the implementation
 * under `shell/src/plugins/nexus/canvas/blockRefDrop.test.ts`.
 */
import '../src/plugins/nexus/canvas/blockRefDrop.test.ts'
