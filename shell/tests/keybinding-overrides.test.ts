/**
 * WI-04 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the KeybindingRegistry override tests
 * that live as a sibling of the implementation under
 * `shell/src/registry/KeybindingRegistry.test.ts`.
 *
 * Same shim pattern as `tests/ai-store.test.ts`.
 */
import '../src/registry/KeybindingRegistry.test.ts'
