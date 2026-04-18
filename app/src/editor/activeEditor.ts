// Module-level registry for the currently-focused CodeMirror editor
// view. Lets cross-cutting commands (palette actions, plugin IPC
// responses) reach "the editor the user is looking at" without
// plumbing a prop through the whole component tree.
//
// The microkernel boundary holds: surfaces publish themselves when
// they gain focus, commands read the latest publication — nothing
// else in the shell needs to know CodeMirror exists.

import type { EditorView } from "@codemirror/view";

let activeView: EditorView | null = null;

export function setActiveEditor(view: EditorView | null): void {
  activeView = view;
}

export function clearActiveEditor(view: EditorView): void {
  if (activeView === view) {
    activeView = null;
  }
}

export function getActiveEditor(): EditorView | null {
  return activeView;
}
