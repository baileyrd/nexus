/**
 * C60 (#413) — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the comments live-refresh matcher tests
 * that live as a sibling of the implementation under
 * `shell/src/plugins/nexus/comments/index.test.ts`. Same pattern as
 * `comments-decode.test.ts`.
 */
import '../src/plugins/nexus/comments/index.test.ts'
