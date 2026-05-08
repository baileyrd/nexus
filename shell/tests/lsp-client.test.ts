/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the BL-077 LSP client tests that live as a sibling of the
 * implementation under
 * `shell/src/plugins/nexus/editor/cm/lspClient.test.ts`.
 */
import '../src/plugins/nexus/editor/cm/lspClient.test.ts'
