/**
 * WI-30f — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the sandbox end-to-end suite that
 * lives as a sibling of the implementation under
 * `shell/src/host/sandbox/sandboxE2E.test.ts`.
 *
 * Same shim pattern as `tests/sandbox-protocol.test.ts` and
 * `tests/sandbox-orchestrator.test.ts`.
 */
import '../src/host/sandbox/sandboxE2E.test.ts'
