/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the src-colocated tests at
 * `src/workspace/workspaceStore.test.ts`.
 *
 * Same shim pattern as `tests/workspace-Leaf.test.ts` — node:test
 * discovers `test()` calls in any imported module so the assertions
 * register as subtests of this wrapper file.
 */
import '../src/workspace/workspaceStore.test.ts'
