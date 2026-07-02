/**
 * C1 (#354) — re-export wrapper so the default `pnpm test` glob picks
 * up the attachment-pipeline tests that live as siblings of the
 * implementation under `shell/src/plugins/nexus/editor/`.
 */
import '../src/plugins/nexus/editor/attachments.test.ts'
