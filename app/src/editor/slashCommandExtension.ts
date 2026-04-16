/**
 * Slash command CodeMirror extension.
 *
 * Triggers an autocomplete popup when the user types `/` at the start
 * of a line or after whitespace. The popup is fuzzy-filterable and
 * inserts a markdown template when a command is selected. Cursor
 * placement inside the template is controlled by a `\0` marker.
 *
 * Implements PRD 08 §6 (MVP subset — plugin-contributed commands are
 * a future slice).
 */

import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
  type CompletionSource,
  insertCompletionText,
} from "@codemirror/autocomplete";
import type { Extension } from "@codemirror/state";

import { fuzzyRank } from "../components/palette/fuzzy";
import { SLASH_COMMANDS, type SlashCommand } from "./slashCommands";

/** Cursor-position placeholder used inside command templates. */
const CURSOR_MARKER = "\0";

/**
 * Build a CodeMirror `Completion` for a slash command. The custom
 * `apply` function replaces the `/query` trigger text with the template
 * and positions the cursor at the first `\0` marker (or at the end if
 * none is present).
 */
function toCompletion(cmd: SlashCommand): Completion {
  return {
    label: cmd.label,
    detail: cmd.description,
    type: cmd.id,
    apply: (view, _completion, from, to) => {
      const template = cmd.template;
      const markerIdx = template.indexOf(CURSOR_MARKER);
      const insertText =
        markerIdx >= 0 ? template.replace(CURSOR_MARKER, "") : template;
      const cursorOffset = markerIdx >= 0 ? markerIdx : insertText.length;

      view.dispatch(insertCompletionText(view.state, insertText, from, to));
      // Position the cursor at the marker location relative to the inserted text.
      const newCursorPos = from + cursorOffset;
      view.dispatch({ selection: { anchor: newCursorPos } });
    },
  };
}

/**
 * Match `/` only when it's at line start or preceded by whitespace.
 * Returns the range covering `/<query>` (without the leading whitespace
 * if any), plus the query string.
 */
const source: CompletionSource = (context: CompletionContext): CompletionResult | null => {
  const match = context.matchBefore(/(^|\s)\/[\w-]*/);
  if (!match) return null;

  const slashIdx = match.text.indexOf("/");
  if (slashIdx < 0) return null;
  const from = match.from + slashIdx;
  const query = match.text.slice(slashIdx + 1); // text after `/`

  const ranked = fuzzyRank(SLASH_COMMANDS, query, (c) =>
    [c.label, ...(c.aliases ?? [])].join(" "),
  );
  // If the user typed a query that matches nothing, dismiss the popup
  // instead of showing an empty list.
  if (ranked.length === 0) return null;

  return {
    from,
    to: context.pos,
    options: ranked.map(({ item }) => toCompletion(item)),
    filter: false, // we already filtered + ranked via fuzzyRank
  };
};

/** CodeMirror extension factory: slash-command autocomplete. */
export function slashCommands(): Extension {
  return autocompletion({
    override: [source],
    // Don't auto-trigger completion on every keystroke — we only want it for `/`.
    activateOnTyping: true,
    // Keep the popup compact.
    maxRenderedOptions: 12,
    // Inject a text badge before the label using the completion's `type`.
    addToOptions: [
      {
        render: (completion) => {
          const cmd = SLASH_COMMANDS.find((c) => c.id === completion.type);
          const span = document.createElement("span");
          span.className = "cm-nx-slash-badge";
          span.textContent = cmd?.badge ?? "";
          return span;
        },
        position: 10,
      },
    ],
  });
}
