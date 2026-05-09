// Wire types for the BL-074 / BL-007 conflict surface.
//
// Source of truth: `crates/nexus-crdt/src/conflict.rs` and
// `crates/nexus-crdt/src/wire.rs`. The Rust side flattens the
// `Conflict` enum (`#[serde(tag = "kind", rename_all = "snake_case")]`)
// into `ConflictDetail`, which adds optional content snapshots. So
// each row on the wire carries the bare-conflict fields (`kind`,
// `block_id`, `local`, `remote` / `delete`, `edit`) alongside the
// resolver-friendly extensions.

import type { BlockId } from '../editor/types'

/** Site + lamport pair identifying a CRDT op. Matches `OpId` in
 *  `crates/nexus-crdt/src/id.rs`. Kept opaque on the shell side —
 *  the resolver modal compares by reference, doesn't manipulate. */
export interface OpId {
  site: string
  lamport: number
}

/** Which side of a merge an op originated from, from the live
 *  session's POV. Mirrors `nexus_crdt::ConflictOrigin`. */
export type ConflictOrigin = 'local' | 'remote'

/** Per-conflict row carried in `ConflictEnvelope.conflicts`. The
 *  flattened wire form means `kind` + `block_id` always appear at the
 *  top level; the variant-specific fields (`local`/`remote` for a
 *  concurrent edit, `delete`/`edit` for a structural conflict) sit
 *  alongside, and the BL-074 additions (`local_content`, `remote_content`,
 *  `delete_origin`) are optional. */
export type ConflictDetail =
  | {
      kind: 'concurrent_block_edit'
      block_id: BlockId
      local: OpId
      remote: OpId
      local_content?: string
      remote_content?: string
    }
  | {
      kind: 'structural_delete_edit'
      block_id: BlockId
      delete: OpId
      edit: OpId
      local_content?: string
      remote_content?: string
      delete_origin?: ConflictOrigin
    }

/** Bus payload published on `com.nexus.editor.crdt.conflict.<relpath>`.
 *  Mirrors `nexus_crdt::wire::ConflictEnvelope`. */
export interface ConflictEnvelope {
  conflicts: ConflictDetail[]
}
