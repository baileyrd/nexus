/**
 * Re-export shim so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the OI-09 store tests that live next to the impl. Mirrors
 * the `editor-store.test.ts` pattern.
 */
import '../src/stores/pluginsStatusStore.test.ts'
