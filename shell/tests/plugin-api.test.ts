/**
 * F-8.1.2 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the host-side `pluginId` binding tests
 * that live as a sibling of the implementation under
 * `shell/src/host/PluginAPI.test.ts`.
 *
 * Same shim pattern as `tests/extension-host.test.ts`.
 */
import '../src/host/PluginAPI.test.ts'
