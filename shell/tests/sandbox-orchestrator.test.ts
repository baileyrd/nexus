/**
 * WI-30d — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the sandbox orchestrator tests that
 * live as a sibling of the implementation under
 * `shell/src/host/sandbox/orchestrator.test.ts`.
 *
 * Same shim pattern as `tests/sandbox-protocol.test.ts`.
 */
import '../src/host/sandbox/orchestrator.test.ts'
