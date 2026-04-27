/**
 * Re-export wrapper so the default `pnpm test` glob picks up the
 * live-preview runtime smoke test that lives next to its implementation
 * under `shell/src/plugins/nexus/editor/cm/livePreview.runtime.test.ts`.
 */
import '../src/plugins/nexus/editor/cm/livePreview.runtime.test.ts'
