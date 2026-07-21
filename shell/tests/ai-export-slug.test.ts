/**
 * #384 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the export-filename slugifier tests
 * that live as a sibling of the implementation under
 * `shell/src/plugins/nexus/ai/aiRuntime.test.ts`. Same pattern as
 * `comments-bus-event.test.ts`.
 */
import '../src/plugins/nexus/ai/aiRuntime.test.ts'
