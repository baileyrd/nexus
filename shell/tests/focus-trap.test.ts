/**
 * SH-005 — re-export wrapper so the default `pnpm --filter nexus-shell test`
 * glob (`tests/*.test.ts`) picks up the useFocusTrap tests that live
 * alongside the implementation under `shell/src/shell/useFocusTrap.test.tsx`.
 */
import '../src/shell/useFocusTrap.test.tsx'
