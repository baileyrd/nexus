/**
 * WI-12 (TS half) — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the terminal stream store unit tests
 * that live as a sibling of the implementation under
 * `shell/src/plugins/nexus/terminal/terminalStore.test.ts`.
 *
 * Same shim pattern as `saved-commands.test.ts` / `ai-store.test.ts`
 * — node:test discovers `test()` calls inside any imported module, so
 * the assertions in the imported file are registered as subtests of
 * this file.
 */
import '../src/plugins/nexus/terminal/terminalStore.test.ts'
