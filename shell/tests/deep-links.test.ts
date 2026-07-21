/**
 * C71 (#424) — wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the deep-link action parser tests that
 * live as a sibling of the implementation. Same pattern as
 * collab-store.test.ts.
 */
import '../src/plugins/nexus/deepLinks/deepLinkAction.test.ts'
