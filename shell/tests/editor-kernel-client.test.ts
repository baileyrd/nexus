/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the editor kernel-client unit tests that live as a sibling
 * of the implementation under
 * `shell/src/plugins/nexus/editor/kernelClient.test.ts`. Same pattern
 * as `editor-store.test.ts` / `comments-decode.test.ts`.
 */
import '../src/plugins/nexus/editor/kernelClient.test.ts'
