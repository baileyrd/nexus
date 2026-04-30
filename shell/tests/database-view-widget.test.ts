/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the database-view-widget unit tests that live as a sibling
 * of the implementation under
 * `shell/src/plugins/nexus/editor/cm/databaseViewWidget.test.ts`.
 * Same pattern as `editor-kernel-client.test.ts`.
 */
import '../src/plugins/nexus/editor/cm/databaseViewWidget.test.ts'
