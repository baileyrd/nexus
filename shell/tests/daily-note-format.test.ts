/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the dailyNotes formatting tests that live as a sibling of
 * the implementation under
 * `shell/src/plugins/nexus/dailyNotes/dailyNoteFormat.test.ts`.
 *
 * Same shim pattern as `tests/api-version-check.test.ts`.
 */
import '../src/plugins/nexus/dailyNotes/dailyNoteFormat.test.ts'
