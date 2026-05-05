/**
 * WI-07 Slice E — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the agent plugin unit tests that live
 * as a sibling of the implementation under
 * `shell/src/plugins/nexus/agent/agent.test.ts`.
 *
 * Same shim pattern as `ai-store.test.ts` / `skills.test.ts` —
 * node:test discovers `test()` calls inside any imported module, so
 * the assertions in the imported file are registered as subtests of
 * this file.
 */
import '../src/plugins/nexus/agent/agent.test.ts'
import '../src/plugins/nexus/agent/aig02.test.ts'
