// shell/tests/semantic-search-merge.test.ts
//
// BL-040 — re-export the co-located tests for the keyword/semantic
// merger so the workspace `pnpm --filter nexus-shell test` runner
// (which globs `tests/*.test.ts`, not plugin-internal `*.test.ts`
// siblings) picks them up. The actual test bodies live next to the
// merger they cover; see shell/src/plugins/nexus/semanticSearch/.
import '../src/plugins/nexus/semanticSearch/merge.test.ts'
