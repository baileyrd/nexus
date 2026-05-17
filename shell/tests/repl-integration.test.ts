/*
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the src-colocated EditorView integration tests at
 * `src/plugins/nexus/editor/cm/replIntegration.test.ts`.
 *
 * Same shim pattern as the other repl-* re-exports in this dir.
 */
import '../src/plugins/nexus/editor/cm/replIntegration.test.ts'
