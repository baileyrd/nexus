/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the database-view-decoration unit tests that live as a
 * sibling of the implementation under
 * `shell/src/plugins/nexus/editor/cm/databaseViewDecorations.test.ts`.
 */
import '../src/plugins/nexus/editor/cm/databaseViewDecorations.test.ts'
