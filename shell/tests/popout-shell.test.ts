/**
 * Re-export wrapper so the default `pnpm --filter nexus-shell test`
 * glob (`tests/*.test.ts`) picks up the PopoutShell unit tests that
 * live alongside the implementation under
 * `shell/src/shell/PopoutShell.test.ts`. Same pattern as
 * `comments-decode.test.ts` / `editor-store.test.ts`.
 */
import '../src/shell/PopoutShell.test.ts'
