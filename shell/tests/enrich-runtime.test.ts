/**
 * Re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the enrich runtime unit tests that
 * live as a sibling of the implementation under
 * `shell/src/plugins/nexus/enrich/enrichRuntime.test.ts`.
 */
import '../src/plugins/nexus/enrich/enrichRuntime.test.ts'
