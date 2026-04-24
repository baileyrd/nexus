/**
 * WI-35 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the CommandRegistry crash-quarantine
 * tests that live as a sibling of the implementation under
 * `shell/src/registry/CommandRegistry.test.ts`.
 *
 * Same shim pattern as `tests/extension-host.test.ts`.
 */
import '../src/registry/CommandRegistry.test.ts'
