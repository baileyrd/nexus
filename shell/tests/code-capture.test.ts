/**
 * Re-export wrapper so the default `pnpm test` glob picks up the
 * BL-046 code-aware capture tests that live alongside the
 * implementation under `shell/src/plugins/nexus/memory/codeCapture.test.ts`.
 */
import '../src/plugins/nexus/memory/codeCapture.test.ts'
