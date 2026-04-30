// TypeScript mirrors of the `com.nexus.comments` kernel wire types.
// Source of truth: `crates/nexus-comments/src/types.rs`.

/** UUID serialized as a lowercase hyphenated string. */
export type CommentId = string
/** UUID serialized as a lowercase hyphenated string. */
export type ThreadId = string
/** UUID stamped onto a markdown block via `com.nexus.editor::stamp_block`. */
export type BlockId = string

/** A single reply within a thread. Mirrors `nexus_comments::types::Comment`. */
export interface Comment {
  id: CommentId
  /** Optional — `None` on the wire when the runtime has no user identity. */
  author?: string
  body: string
  /** Mentions extracted at write time. Empty array equivalent to absent. */
  mentions: string[]
  /** RFC3339 timestamp. */
  created_at: string
  /** Set when edited in place. */
  updated_at?: string
}

/** A thread anchored to one block in one file. */
export interface Thread {
  id: ThreadId
  block_id: BlockId
  resolved: boolean
  resolved_at?: string
  resolved_by?: string
  created_at: string
  /** Always non-empty: the first comment is created with the thread. */
  comments: Comment[]
}
