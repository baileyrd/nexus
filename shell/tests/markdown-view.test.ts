/**
 * #405 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the MarkdownView unit tests that live
 * as a sibling of the implementation under
 * `shell/src/plugins/nexus/editor/MarkdownView.test.ts`.
 *
 * The sibling file is the canonical location (matches the editorStore
 * pattern). This wrapper exists purely so CI runs them automatically —
 * node:test discovers `test()` calls inside any imported module, so
 * the assertions in the imported file are registered as subtests of
 * this file.
 */
import '../src/plugins/nexus/editor/MarkdownView.test.ts'
