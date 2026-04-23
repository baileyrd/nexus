/**
 * WI-13 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the UriHandlerRegistry tests that live
 * as a sibling of the implementation under
 * `shell/src/registry/UriHandlerRegistry.test.ts`.
 *
 * Same shim pattern as `tests/keybinding-overrides.test.ts`.
 */
import '../src/registry/UriHandlerRegistry.test.ts'
