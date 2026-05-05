/**
 * BL-044 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the recall plugin's unit tests, which
 * live as siblings of the implementation under
 * `shell/src/plugins/nexus/recall/`.
 *
 * Same pattern as `tests/cmd-i-overlay.test.ts` and
 * `tests/memory-capture-store.test.ts`.
 */
import '../src/plugins/nexus/recall/recallStore.test.ts'
import '../src/plugins/nexus/recall/insertFormat.test.ts'
import '../src/plugins/nexus/recall/recallRuntime.test.ts'
import '../src/plugins/nexus/recall/recallHotkey.test.ts'
import '../src/plugins/nexus/recall/highlightRuns.test.ts'
