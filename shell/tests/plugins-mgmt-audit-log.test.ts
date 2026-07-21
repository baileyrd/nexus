/**
 * C84 (#437) — wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the audit-log helper tests that live
 * as a sibling of the implementation. Same pattern as
 * collab-store.test.ts.
 */
import '../src/plugins/nexus/pluginsMgmt/AuditLogModal.test.ts'
