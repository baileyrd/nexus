/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the memoryDashboard tests that live as a sibling of the
 * implementation under `shell/src/plugins/nexus/memoryDashboard/index.test.ts`.
 *
 * Same shim pattern as `tests/api-version-check.test.ts` — this one was
 * missed when the C35 (#388) memory forget/edit UX landed; adding it now
 * closes the gap (the tests always passed standalone, they just never ran
 * in `pnpm test`/CI).
 */
import '../src/plugins/nexus/memoryDashboard/index.test.ts'
