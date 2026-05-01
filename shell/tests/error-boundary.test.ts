/**
 * SH-001 — re-export wrapper so the default `pnpm --filter nexus-shell test`
 * glob (`tests/*.test.ts`) picks up the ErrorBoundary render tests that
 * live alongside the implementation under
 * `shell/src/shell/ErrorBoundary.test.tsx`.
 *
 * Same pattern as `tests/popout-shell.test.ts`.
 */
import '../src/shell/ErrorBoundary.test.tsx'
