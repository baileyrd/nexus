/**
 * CodeMirror 6 snippet extension (PRD-08 §14 plugin contribution).
 *
 * When the user presses Tab, this extension checks whether the text
 * immediately before the cursor matches a registered snippet trigger.
 * On match it:
 *   1. Deletes the trigger text.
 *   2. Inserts the snippet body (with `$CURSOR` stripped).
 *   3. Positions the cursor at `$CURSOR`, or at the end of the body.
 *
 * File-type restrictions (`snippet.fileTypes`) are enforced by the
 * `buildSnippetExtension` factory — the returned extension is already
 * filtered for the active file type so the keymap handler stays O(n)
 * over matching snippets only.
 */

import { keymap } from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { Snippet } from "../contributions";

const CURSOR_MARKER = "$CURSOR";

/**
 * Attempt to expand the snippet whose trigger immediately precedes the
 * cursor. Returns `true` (consumed) on success, `false` to let CM6
 * handle Tab normally (e.g. indent).
 */
function tryExpandSnippet(view: EditorView, activeSnippets: Snippet[]): boolean {
  const { state } = view;
  const { head } = state.selection.main;
  // Only expand when no text is selected.
  if (!state.selection.main.empty) return false;

  const line = state.doc.lineAt(head);
  const textBeforeCursor = line.text.slice(0, head - line.from);

  for (const snippet of activeSnippets) {
    if (!textBeforeCursor.endsWith(snippet.trigger)) continue;

    const triggerStart = head - snippet.trigger.length;
    const markerIdx = snippet.body.indexOf(CURSOR_MARKER);
    const insertText =
      markerIdx >= 0 ? snippet.body.replace(CURSOR_MARKER, "") : snippet.body;
    const cursorOffset = markerIdx >= 0 ? markerIdx : insertText.length;

    view.dispatch({
      changes: { from: triggerStart, to: head, insert: insertText },
      selection: { anchor: triggerStart + cursorOffset },
    });
    return true;
  }
  return false;
}

/**
 * Build a CodeMirror extension that expands snippets on Tab.
 *
 * `activeSnippets` should already be filtered to the current file type by
 * the caller. The extension captures a reference to the array reference
 * itself — the caller must replace the compartment when the snippet list
 * changes (see `EditorSurface.tsx`).
 */
export function buildSnippetExtension(activeSnippets: Snippet[]): Extension {
  if (activeSnippets.length === 0) return [];
  return keymap.of([
    {
      key: "Tab",
      run: (view) => tryExpandSnippet(view, activeSnippets),
    },
  ]);
}

/** Filter `snippets` to those active for `fileExt` (case-insensitive). */
export function filterSnippetsForExt(snippets: Snippet[], fileExt: string): Snippet[] {
  const ext = fileExt.toLowerCase().replace(/^\./, "");
  return snippets.filter(
    (s) => !s.fileTypes || s.fileTypes.length === 0 || s.fileTypes.includes(ext),
  );
}
