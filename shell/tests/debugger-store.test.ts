/**
 * BL-081 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the debugger-store unit tests
 * colocated with the implementation.
 */
import '../src/plugins/nexus/debugger/debuggerStore.test.ts'
