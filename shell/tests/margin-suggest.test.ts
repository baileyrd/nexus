/**
 * BL-036 — re-export wrapper so the default `pnpm test` glob
 * (`tests/*.test.ts`) picks up the AMB margin-suggestions unit tests
 * that live as siblings of the implementation under
 * `shell/src/plugins/nexus/ai/` (phase 1 — engine + store) and
 * `shell/src/plugins/nexus/editor/cm/` (phase 2 — decoration +
 * accept/dismiss helpers).
 *
 * Mirrors `tests/cmd-i-overlay.test.ts` — node:test discovers
 * `test()` calls inside imported modules, so the assertions in the
 * imported files register as subtests of this file.
 */
import '../src/plugins/nexus/ai/marginSuggestStore.test.ts'
import '../src/plugins/nexus/ai/marginSuggest.test.ts'
import '../src/plugins/nexus/editor/cm/marginSuggestions.test.ts'
import '../src/plugins/nexus/editor/cm/marginSuggestTrigger.test.ts'
