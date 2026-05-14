/**
 * BL-118 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the audio plugin's unit tests, which
 * live as siblings of the implementation under
 * `shell/src/plugins/nexus/audio/`.
 *
 * Same pattern as `tests/recall.test.ts`.
 */
import '../src/plugins/nexus/audio/speechApi.test.ts'
import '../src/plugins/nexus/audio/runtime.test.ts'
