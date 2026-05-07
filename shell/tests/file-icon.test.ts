/**
 * Re-export wrapper so the default `pnpm test` glob (`tests/*.test.ts`)
 * picks up the BL-080 file-icon-mapping tests that live as a sibling
 * of the implementation.
 */
import '../src/plugins/nexus/files/fileIcon.test.ts'
