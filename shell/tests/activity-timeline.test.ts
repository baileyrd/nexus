/**
 * AIG-04 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the colocated activityTimeline store
 * tests under `shell/src/plugins/nexus/activityTimeline/`.
 *
 * Same shim pattern as `agent.test.ts` / `skills.test.ts`.
 */
import '../src/plugins/nexus/activityTimeline/activityTimelineStore.test.ts'
