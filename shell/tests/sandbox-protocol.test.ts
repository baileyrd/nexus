/**
 * WI-30b — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the sandbox RPC protocol tests that
 * live as a sibling of the implementation under
 * `shell/src/host/sandbox/sandboxProtocol.test.ts`.
 *
 * Same shim pattern as `tests/extension-host.test.ts` +
 * `tests/uri-handler-registry.test.ts`.
 */
import '../src/host/sandbox/sandboxProtocol.test.ts'
