/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the taskDashboard grouping tests that live as a sibling of
 * the implementation under
 * `shell/src/plugins/nexus/taskDashboard/taskGrouping.test.ts`.
 *
 * Same shim pattern as `tests/api-version-check.test.ts`.
 */
import '../src/plugins/nexus/taskDashboard/taskGrouping.test.ts'
