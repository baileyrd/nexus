/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the src-colocated tests at
 * `src/plugins/nexus/noteContext/backlinksLoader.test.ts`.
 */
import '../src/plugins/nexus/noteContext/backlinksLoader.test.ts'
