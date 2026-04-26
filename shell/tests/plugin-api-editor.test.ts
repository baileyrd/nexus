/**
 * Re-export shim so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the OI-14 PluginAPI editor projection tests that live next
 * to the impl. Mirrors the `editor-store.test.ts` pattern.
 */
import '../src/host/PluginAPI.editor.test.ts'
