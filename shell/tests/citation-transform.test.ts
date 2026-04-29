/**
 * BL-038 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the citation-transform unit tests
 * that live as a sibling of the implementation under
 * `shell/src/plugins/nexus/ai/citationTransform.test.ts`.
 *
 * Mirrors the pattern used by `tests/ai-store.test.ts`.
 */
import '../src/plugins/nexus/ai/citationTransform.test.ts'
