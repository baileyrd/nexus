// TypeScript mirrors of the `com.nexus.editor` kernel wire types.
//
// Source of truth: `crates/nexus-editor/src/{transaction,annotation,block,tree,core_plugin}.rs`.
// Keep these in sync with serde attributes — most enums use
// `#[serde(rename_all = "snake_case", tag = "kind")]` so on the wire they
// are tagged unions with a `kind` discriminator and snake_case field
// names. `EditorSnapshot` itself is `rename_all = "camelCase"`.
//
// Scope note: `Block` and `BlockTree` are typed precisely enough for the
// fields the kernel client actually produces/consumes. Deep block-tree
// manipulation happens on the Rust side; the UI layer mostly threads
// snapshots through.

// ── Primitive aliases ────────────────────────────────────────────────────────

/** UUID serialized as a lowercase hyphenated string. */
export type BlockId = string

/**
 * UUID of a `Transaction` (mirrors `Transaction.id` on the wire).
 * Kept as an alias so echo-suppression call sites (`editorStore
 * .pendingLocalRevisions`, `sessionManager`) read as intent rather
 * than as a bare `string`. Phase 4 of the editor transaction wiring
 * plan.
 */
export type TransactionId = string

// ── Annotation (mirrors nexus-editor/src/annotation.rs) ──────────────────────

/**
 * Tagged union with `kind` discriminator — mirrors `AnnotationType` in
 * `annotation.rs:47`. `#[serde(rename_all = "snake_case", tag = "kind")]`
 * means unit variants serialize as `{ "kind": "bold" }`, tuple variants
 * flatten their single field into a named field, and struct variants
 * inline their fields alongside `kind`.
 */
export type AnnotationType =
  | { kind: 'bold' }
  | { kind: 'italic' }
  | { kind: 'strikethrough' }
  | { kind: 'underline' }
  | { kind: 'code' }
  // Tuple variants — serde flattens the single payload as the variant
  // name (snake_case). See `TextColor(String)` / `HighlightColor(String)`.
  | { kind: 'text_color'; text_color: string }
  | { kind: 'highlight_color'; highlight_color: string }
  | {
      kind: 'link'
      url: string
      title: string | null
    }
  | {
      kind: 'wikilink'
      path: string
      display_text: string | null
      is_resolved: boolean
    }
  | {
      kind: 'mention'
      user_id: string
      display_name: string
    }
  | {
      kind: 'math_inline'
      formula: string
    }
  | {
      kind: 'block_ref'
      block_id: BlockId
    }
  | {
      kind: 'custom'
      plugin_id: string
      ty: string
      data: Record<string, unknown>
    }

/** Inline formatting range over a block's `content`. Mirrors `Annotation`. */
export interface Annotation {
  /** Inclusive start byte within the block's `content`. */
  start: number
  /** Exclusive end byte within the block's `content`. */
  end: number
  /** Type + payload. */
  ty: AnnotationType
}

// ── Block / tree (mirrors nexus-editor/src/{block,tree}.rs) ──────────────────

/**
 * Block type discriminant. Rust uses
 * `#[serde(rename_all = "snake_case", tag = "kind")]`. Variants carry
 * type-specific payload fields alongside `kind`. Kept as
 * `{ kind: string; [k: string]: unknown }` so the client does not pin
 * down every variant — downstream views narrow as needed.
 */
export interface BlockType {
  kind: string
  [extra: string]: unknown
}

/**
 * Rich block properties (`BlockProperties`). The Rust type is a map of
 * string → `PropertyValue`; typed permissively here.
 */
export type BlockProperties = Record<string, unknown>

/** Mirrors `Block` in `block.rs:22` (no serde rename — snake_case by field name). */
export interface Block {
  id: BlockId
  ty: BlockType
  content: string
  annotations: Annotation[]
  properties: BlockProperties
  parent_id: BlockId | null
  children: BlockId[]
  index_in_parent: number
  created_at: number
  updated_at: number
  is_deleted: boolean
}

/** Document-level metadata (`DocumentMetadata`). Typed permissively. */
export type DocumentMetadata = Record<string, unknown>

/** Mirrors `BlockTree` in `tree.rs:26`. */
export interface BlockTree {
  blocks: Record<BlockId, Block>
  root_blocks: BlockId[]
  metadata: DocumentMetadata
}

// ── Operation (mirrors transaction.rs:22) ────────────────────────────────────

/**
 * Tagged union with `kind` discriminator —
 * `#[serde(rename_all = "snake_case", tag = "kind")]` on
 * `enum Operation`. Field names are snake_case.
 */
export type Operation =
  | {
      kind: 'insert_text'
      block_id: BlockId
      pos: number
      text: string
      pre_annotations: Annotation[]
    }
  | {
      kind: 'delete_text'
      block_id: BlockId
      pos: number
      deleted_text: string
      pre_annotations: Annotation[]
    }
  | {
      kind: 'insert_block'
      block: Block
      parent_id: BlockId | null
      index_in_parent: number
    }
  | {
      kind: 'delete_block'
      old_block: Block
      was_parent_id: BlockId | null
      was_index_in_parent: number
    }
  | {
      kind: 'reparent_block'
      id: BlockId
      old_parent_id: BlockId | null
      old_index_in_parent: number
      new_parent_id: BlockId | null
      new_index_in_parent: number
    }
  | {
      kind: 'update_block_content'
      id: BlockId
      old_content: string
      new_content: string
      old_annotations: Annotation[]
      new_annotations: Annotation[]
    }
  | {
      kind: 'update_annotations'
      block_id: BlockId
      old_annotations: Annotation[]
      new_annotations: Annotation[]
    }

// ── Transaction metadata (mirrors transaction.rs:392/416/445) ────────────────

/**
 * Block-tree operation kinds (`BlockOp` in `transaction.rs:416`). Tagged
 * union with `kind` discriminator.
 */
export type BlockOp =
  | { kind: 'create'; block_type: string }
  | { kind: 'delete' }
  | { kind: 'move'; direction: string }
  | { kind: 'transform'; from_type: string; to_type: string }
  | { kind: 'indent' }
  | { kind: 'unindent' }

/**
 * High-level user gesture (`UserAction` in `transaction.rs:392`). Tagged
 * union with `kind` discriminator.
 */
export type UserAction =
  | { kind: 'keystroke' }
  | { kind: 'paste' }
  | { kind: 'delete' }
  | { kind: 'slash_command'; command: string }
  | { kind: 'block_operation'; op: BlockOp }
  | { kind: 'drag_drop' }

/**
 * Who originated a transaction (`TransactionSource` in
 * `transaction.rs:445`). `#[serde(rename_all = "snake_case")]` with no
 * `tag` on a unit-variant-only enum — the wire form is the bare string
 * `"user" | "ai" | "sync" | "system"`.
 */
export type TransactionSource = 'user' | 'ai' | 'sync' | 'system'

/** Mirrors `TransactionMetadata` in `transaction.rs:371`. */
export interface TransactionMetadata {
  user_action: UserAction
  source: TransactionSource
  ai_edit: boolean
}

// ── Transaction (mirrors transaction.rs:317) ─────────────────────────────────

/**
 * Atomic, reversible group of `Operation`s. `created_at` is Unix epoch
 * milliseconds.
 */
export interface Transaction {
  /** UUID (hyphenated) — `UndoTree` uses this to identify history nodes. */
  id: string
  operations: Operation[]
  created_at: number
  metadata: TransactionMetadata
}

// ── EditorSnapshot (mirrors core_plugin.rs:78) ───────────────────────────────

/**
 * Snapshot of an open editor session. The Rust struct is tagged
 * `#[serde(rename_all = "camelCase")]`, so on the wire field names are
 * camelCase unlike the rest of the editor types.
 *
 * Cited line numbers refer to `crates/nexus-editor/src/core_plugin.rs`:
 * - `relpath` (line 80)
 * - `tree` (line 82)
 * - `undoPosition` — `Option<usize>`; `null` means "at the virtual root" (lines 84–85)
 * - `undoLen` (line 87)
 * - `canUndo` (line 89)
 * - `canRedo` (line 91)
 */
export interface EditorSnapshot {
  relpath: string
  tree: BlockTree
  undoPosition: number | null
  undoLen: number
  canUndo: boolean
  canRedo: boolean
  /**
   * Monotonic per-session mutation counter. Bumped by the Rust plugin
   * on every successful `apply_transaction`/`undo`/`redo`/`sync_content`
   * before the snapshot is built. Shell subscribers compare against
   * `editorStore.sessionRevision` to detect stale state and drive
   * echo suppression via `pendingLocalRevisions`. Mirrors
   * `EditorSnapshot.revision` in `core_plugin.rs`. Phase 4 of the
   * editor transaction wiring plan.
   */
  revision: number
}

// ── Changed-event payload (mirrors `publish_changed` in core_plugin.rs) ──────

/**
 * Payload carried by the `com.nexus.editor.changed.<relpath>` custom
 * event. Rust publishes the JSON object
 * `{ relpath, revision, transaction_id }`; `transaction_id` is the
 * UUID of the applied transaction for `apply_transaction` events and
 * `null` for `undo`/`redo`/`sync_content` mutations. Phase 4.
 */
export interface EditorChangedPayload {
  relpath: string
  revision: number
  transaction_id: TransactionId | null
}
