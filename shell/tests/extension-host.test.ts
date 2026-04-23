/**
 * WI-19 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the ExtensionHost activation-event tests
 * that live as a sibling of the implementation under
 * `shell/src/host/ExtensionHost.test.ts`.
 *
 * Same shim pattern as `tests/uri-handler-registry.test.ts`.
 */
import '../src/host/ExtensionHost.test.ts'
