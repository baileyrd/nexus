// Typed wrappers for the editor core plugin Tauri commands.
//
// These forward straight to `com.nexus.editor` via kernel IPC
// (see `crates/nexus-app/src/editor.rs`). Types are hand-written
// for now because the serialized `BlockTree` / `Transaction` shapes
// are large and will get proper ts-rs bindings in a follow-up.

import { invoke } from "@tauri-apps/api/core";

/** Snapshot of an open editor session. */
export interface EditorSnapshot {
  /** Forge-relative path of the session. */
  relpath: string;
  /**
   * Full block tree. Typed as `unknown` until ts-rs bindings land —
   * callers should narrow via their own runtime type guard.
   */
  tree: unknown;
  /**
   * Current undo-tree cursor. `null` means "virtual root" (nothing
   * applied yet, or fully undone).
   */
  undoPosition: number | null;
  /** Total number of transactions recorded in history. */
  undoLen: number;
  /** `true` if `editorUndo` would change state. */
  canUndo: boolean;
  /** `true` if `editorRedo` would change state. */
  canRedo: boolean;
}

/** Parse a markdown file and create an in-memory editor session. */
export function editorOpen(relpath: string): Promise<EditorSnapshot> {
  return invoke<EditorSnapshot>("editor_open", { relpath });
}

/** Drop an editor session without saving. */
export function editorClose(relpath: string): Promise<void> {
  return invoke<void>("editor_close", { relpath });
}

/** Fetch a fresh snapshot of an open session. */
export function editorGetTree(relpath: string): Promise<EditorSnapshot> {
  return invoke<EditorSnapshot>("editor_get_tree", { relpath });
}

/** Serialize the in-memory tree back to disk. */
export function editorSave(relpath: string): Promise<void> {
  return invoke<void>("editor_save", { relpath });
}

/** Apply a serialized `Transaction` atomically. */
export function editorApplyTransaction(
  relpath: string,
  transaction: unknown,
): Promise<EditorSnapshot> {
  return invoke<EditorSnapshot>("editor_apply_transaction", {
    relpath,
    transaction,
  });
}

/** Undo the most recent applied transaction. */
export function editorUndo(relpath: string): Promise<EditorSnapshot> {
  return invoke<EditorSnapshot>("editor_undo", { relpath });
}

/** Redo the most recent undone transaction. */
export function editorRedo(relpath: string): Promise<EditorSnapshot> {
  return invoke<EditorSnapshot>("editor_redo", { relpath });
}

/** Forge-relative paths of currently-open sessions, sorted. */
export function editorListOpen(): Promise<string[]> {
  return invoke<string[]>("editor_list_open");
}
