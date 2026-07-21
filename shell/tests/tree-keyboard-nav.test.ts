/**
 * C73 (#426) — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the file-tree arrow-key navigation
 * index-math tests that live as a sibling of the implementation under
 * `shell/src/plugins/nexus/files/treeKeyboardNav.test.ts`.
 *
 * Same shim pattern as `tests/tab-context-menu.test.ts`.
 */
import '../src/plugins/nexus/files/treeKeyboardNav.test.ts'
