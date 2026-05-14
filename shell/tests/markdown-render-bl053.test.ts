/**
 * BL-053 Phases 2/3/4 — re-export wrapper so the default
 * `pnpm test` glob picks up the live-preview pipeline tests
 * which live as siblings of the implementation under
 * `shell/src/plugins/nexus/editor/`.
 */
import '../src/plugins/nexus/editor/markdownRender.test.ts'
