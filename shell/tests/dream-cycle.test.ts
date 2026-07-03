/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the dreamCycle composeToast tests that live as a sibling of
 * the implementation under
 * `shell/src/plugins/nexus/dreamCycle/index.test.ts`.
 *
 * Same shim pattern as `tests/api-version-check.test.ts`.
 */
import '../src/plugins/nexus/dreamCycle/index.test.ts'
