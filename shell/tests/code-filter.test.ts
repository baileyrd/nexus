/**
 * Re-export wrapper so the default `pnpm test` glob picks up the
 * BL-046 phase-2 recall filter tests living alongside the
 * implementation under `shell/src/plugins/nexus/recall/codeFilter.test.ts`.
 */
import '../src/plugins/nexus/recall/codeFilter.test.ts'
