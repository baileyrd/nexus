/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the src-colocated tests at `src/plugins/nexus/ai/actions/builtins.test.ts`.
 *
 * Same shim pattern as `tests/bases-store.test.ts` etc — node:test
 * discovers `test()` calls in any imported module so the assertions
 * register as subtests of this wrapper file. Wrapper-style import is
 * needed because shell/ isn't `"type": "module"` and direct runs of
 * src tests with top-level `await` hit an esbuild CJS transform error.
 */
import '../src/plugins/nexus/ai/actions/builtins.test.ts'
