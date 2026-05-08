/**
 * BL-093 follow-up — re-export wrapper so the default `pnpm test`
 * glob (`tests/*.test.ts`) picks up the health-panel metrics
 * formatter unit tests that live as a sibling of the implementation
 * under `shell/src/plugins/nexus/healthPanel/metricsFormat.test.ts`.
 *
 * Same shim pattern as `saved-commands.test.ts`.
 */
import '../src/plugins/nexus/healthPanel/metricsFormat.test.ts'
