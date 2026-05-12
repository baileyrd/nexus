/**
 * DG-25 — re-export wrapper so the default `pnpm --filter nexus-shell test`
 * glob (`tests/*.test.ts`) picks up the popoutCompatible contract test
 * that lives alongside the catalog under `shell/src/plugins/`.
 */
import '../src/plugins/popoutCompatible.test.ts'
