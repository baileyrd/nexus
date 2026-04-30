/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the comments decoder unit tests that live as a sibling of
 * the implementation under
 * `shell/src/plugins/nexus/comments/decode.test.ts`. Same pattern as
 * `editor-store.test.ts` / `outline-parse.test.ts`.
 */
import '../src/plugins/nexus/comments/decode.test.ts'
