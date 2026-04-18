// Inline AI completion for the markdown editor (PRD-08 §9).
//
// Palette command flow:
//  1. Grab the currently-focused CodeMirror view.
//  2. Collect context — either the active selection, or up to
//     `CONTEXT_CHARS` preceding the cursor when nothing is selected.
//  3. Open a stream_chat session against `com.nexus.ai` via the host
//     shell's `ai_stream_chat` command.
//  4. Insert each streamed chunk at a tracked anchor position using a
//     CM6 transaction. The anchor advances with each insert so the
//     assistant's output stays coherent even across many chunks.
//
// This is the thinnest useful slice — no ghost-text widget, no
// accept/reject affordance beyond Ctrl+Z. A later slice can layer
// a preview decoration on top of the same machinery.

import { EditorSelection, type ChangeSpec } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

import {
  aiStreamChat,
  onAiStreamChunk,
  onAiStreamDone,
  type ChatMessage,
} from "../ipc/ai";
import { getActiveEditor } from "./activeEditor";

/** Chars of context pulled from the document when the user's selection
 *  is empty. Big enough to be useful, small enough to stay fast on
 *  remote providers. */
const CONTEXT_CHARS = 2_000;

const SYSTEM_PROMPT =
  "You are an inline writing assistant embedded in a markdown editor. " +
  "Continue the user's document at the insertion point. Return only the " +
  "new text — no commentary, no code fences, no restatement of what the " +
  "user already wrote. Preserve the surrounding tone and formatting.";

function makeSessionId(): string {
  return `inline-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

interface Insertion {
  view: EditorView;
  sessionId: string;
  /** Current insertion anchor. Advances as chunks stream in. */
  pos: number;
  /** Tokens buffered while the async listeners are being wired. */
  pendingChunks: string[];
  /** Resolved once the session actually starts, so buffered chunks
   *  flush in order. */
  started: boolean;
}

function insert(insertion: Insertion, text: string): void {
  if (text.length === 0) return;
  const { view } = insertion;
  const from = insertion.pos;
  const change: ChangeSpec = { from, to: from, insert: text };
  const tx = view.state.update({
    changes: change,
    selection: EditorSelection.cursor(from + text.length),
    scrollIntoView: true,
  });
  view.dispatch(tx);
  insertion.pos = from + text.length;
}

function buildMessages(view: EditorView): { messages: ChatMessage[] } {
  const doc = view.state.doc;
  const selection = view.state.selection.main;
  let contextText: string;
  if (!selection.empty) {
    contextText = doc.sliceString(selection.from, selection.to);
  } else {
    const from = Math.max(0, selection.from - CONTEXT_CHARS);
    contextText = doc.sliceString(from, selection.from);
  }
  const instruction = selection.empty
    ? "Continue this document:"
    : "Rewrite or continue the selected text:";
  const content = `${instruction}\n\n${contextText}`;
  return {
    messages: [{ role: "user", content }],
  };
}

/** Run one inline AI completion against the currently-focused editor.
 *  Resolves when the provider signals end-of-stream or rejects on any
 *  provider error. */
export async function runInlineAi(): Promise<void> {
  const view = getActiveEditor();
  if (!view) {
    throw new Error("No editor focused — click into a document first.");
  }

  const sessionId = makeSessionId();
  // Anchor the first insertion at the end of the current selection so
  // the streamed text lands right where the user is looking. When the
  // selection is non-empty we replace it up front and insert after.
  const sel = view.state.selection.main;
  let startPos = sel.to;
  if (!sel.empty) {
    view.dispatch({
      changes: { from: sel.from, to: sel.to, insert: "" },
      selection: EditorSelection.cursor(sel.from),
    });
    startPos = sel.from;
  }

  const insertion: Insertion = {
    view,
    sessionId,
    pos: startPos,
    pendingChunks: [],
    started: false,
  };

  const unlistenChunk = await onAiStreamChunk((ev) => {
    if (ev.session_id !== sessionId) return;
    if (!insertion.started) {
      insertion.pendingChunks.push(ev.chunk);
      return;
    }
    insert(insertion, ev.chunk);
  });

  let resolveDone: (() => void) | null = null;
  const done = new Promise<void>((resolve) => {
    resolveDone = resolve;
  });

  const unlistenDone = await onAiStreamDone((ev) => {
    if (ev.session_id !== sessionId) return;
    resolveDone?.();
  });

  insertion.started = true;
  // Flush anything the listener captured before we flipped `started`.
  if (insertion.pendingChunks.length > 0) {
    const buffered = insertion.pendingChunks.join("");
    insertion.pendingChunks = [];
    insert(insertion, buffered);
  }

  const { messages } = buildMessages(view);

  try {
    await aiStreamChat(messages, { sessionId, system: SYSTEM_PROMPT });
    // The backend's stream_chat handler only resolves after `stream_done`
    // fires, but we still await the done-event promise so any final
    // chunks that arrived slightly after the handler return get
    // flushed before cleanup.
    await Promise.race([
      done,
      new Promise<void>((r) => setTimeout(r, 50)),
    ]);
  } finally {
    unlistenChunk();
    unlistenDone();
  }
}
