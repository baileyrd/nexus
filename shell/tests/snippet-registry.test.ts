/**
 * OI-18 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the SnippetRegistry tests that live
 * as a sibling of the implementation under
 * `shell/src/registry/SnippetRegistry.test.ts`.
 *
 * Same shim pattern as `tests/command-registry.test.ts`.
 */
import '../src/registry/SnippetRegistry.test.ts'
