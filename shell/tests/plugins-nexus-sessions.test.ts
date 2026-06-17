/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the src-colocated tests at
 * `src/plugins/nexus/sessions/sessions.test.ts` (RFC 0008, Phase 5.4).
 *
 * Same shim pattern as `tests/plugins-nexus-outline-parse.test.ts` etc —
 * node:test discovers `test()` calls in any imported module, so the
 * session-tree assertions register as subtests of this wrapper file.
 */
import '../src/plugins/nexus/sessions/sessions.test.ts'
