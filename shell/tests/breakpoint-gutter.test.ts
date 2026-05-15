/*
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the src-colocated breakpoint-gutter tests at
 * `src/plugins/nexus/editor/cm/breakpointGutter.test.ts`.
 *
 * Same shim pattern as `tests/conflict-parser.test.ts` and friends.
 */
import '../src/plugins/nexus/editor/cm/breakpointGutter.test.ts'
