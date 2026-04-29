// shell/tests/link-suggest.test.ts
//
// BL-039 — re-export wrapper so the default `pnpm test` glob
// (`tests/*.test.ts`) picks up the link-suggestion tests that live
// next to the implementation under
// `shell/src/plugins/nexus/editor/cm/linkSuggest.test.ts`. Mirrors
// the pattern set by ghost-completion.test.ts and
// semantic-search-merge.test.ts.
import '../src/plugins/nexus/editor/cm/linkSuggest.test.ts'
