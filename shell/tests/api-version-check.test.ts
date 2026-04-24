/**
 * WI-33 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the apiVersion check tests that live as
 * a sibling of the implementation under
 * `shell/src/host/communityPluginLoader.test.ts`.
 *
 * Same shim pattern as `tests/extension-host.test.ts` +
 * `tests/uri-handler-registry.test.ts`.
 */
import '../src/host/communityPluginLoader.test.ts'
