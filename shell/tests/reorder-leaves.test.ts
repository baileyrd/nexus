/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the workspace.reorderLeaves tests that live alongside the
 * implementation under `shell/src/workspace/reorderLeaves.test.ts`.
 *
 * Same shim pattern as `tests/snippet-registry.test.ts`.
 */
import '../src/workspace/reorderLeaves.test.ts'
