/**
 * Re-export shim so the default `pnpm test` glob picks up the BL-008
 * fenced-code-registry tests that live next to the implementation
 * under `shell/src/plugins/nexus/editor/cm/fencedCodeRegistry.test.ts`.
 */
import '../src/plugins/nexus/editor/cm/fencedCodeRegistry.test.ts'
