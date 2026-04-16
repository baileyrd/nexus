/**
 * CodeMirror 6 editor surface.
 *
 * Wraps an EditorView instance with markdown syntax highlighting,
 * Mod+S save, and theme integration via CSS variables.
 *
 * Dynamic extension slots (PRD-08 §14.2/§14.3) are held in CM6
 * `Compartment`s so plugin-contributed decoration providers and
 * editor-scoped keybindings can be swapped in/out without rebuilding
 * the editor state. The surface subscribes to the contribution registry
 * and reconfigures each compartment as registrations change.
 */

import { useEffect, useRef, useCallback, useMemo } from "react";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine, drawSelection, type KeyBinding } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { markdown } from "@codemirror/lang-markdown";
import { bracketMatching, foldGutter, indentOnInput, syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
import { liveMarkdown } from "../../editor/liveMarkdown";
import { slashCommands } from "../../editor/slashCommandExtension";
import { buildSnippetExtension, filterSnippetsForExt } from "../../editor/snippetExtension";
import { contributions, type EditorKeybinding } from "../../contributions";

export interface EditorSurfaceProps {
  initialContent: string;
  filePath: string;
  onChange?: (content: string) => void;
  onSave?: (content: string) => void;
}

/** CodeMirror theme that reads from Nexus CSS variables. */
const nexusEditorTheme = EditorView.theme({
  "&": {
    height: "100%",
    fontSize: "14px",
    fontFamily:
      "ui-monospace, SFMono-Regular, Menlo, monospace, " +
      "'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji', 'Twemoji Mozilla', emoji",
  },
  "&.cm-focused": {
    outline: "none",
  },
  ".cm-scroller": {
    overflow: "auto",
  },
  ".cm-gutters": {
    backgroundColor: "var(--nx-bg-secondary, #f5f5f5)",
    color: "var(--nx-text-tertiary, #9a9ca4)",
    border: "none",
    paddingRight: "4px",
  },
  ".cm-activeLineGutter": {
    backgroundColor: "var(--nx-bg-tertiary, #e8eaef)",
  },
  ".cm-activeLine": {
    backgroundColor: "var(--nx-bg-tertiary, rgba(0,0,0,0.04))",
  },
  ".cm-selectionBackground, ::selection": {
    backgroundColor: "var(--nx-accent-muted, rgba(79, 143, 247, 0.2)) !important",
  },
  ".cm-cursor": {
    borderLeftColor: "var(--nx-text-primary, #111)",
  },
});

/** Convert a registered editor keybinding to a CM6 KeyBinding that
 *  dispatches through the contribution registry. */
function toCmKeyBinding(b: EditorKeybinding): KeyBinding {
  return {
    key: b.key,
    run: () => {
      contributions.invokeCommand(b.commandId);
      return true;
    },
  };
}

function currentPluginDecorations(): Extension[] {
  return contributions.listEditorDecorationProviders().map((p) => p.extension);
}

function currentPluginKeymap(): Extension {
  return keymap.of(contributions.listEditorKeybindings().map(toCmKeyBinding));
}

function currentSnippetExtension(filePath: string) {
  const ext = filePath.split(".").pop()?.toLowerCase() ?? "";
  return buildSnippetExtension(filterSnippetsForExt(contributions.listSnippets(), ext));
}

function getExtensions(
  filePath: string,
  onChangeRef: React.MutableRefObject<EditorSurfaceProps["onChange"]>,
  onSaveRef: React.MutableRefObject<EditorSurfaceProps["onSave"]>,
  pluginDecorationsCompartment: Compartment,
  pluginKeymapCompartment: Compartment,
  snippetCompartment: Compartment,
) {
  const ext = filePath.split(".").pop()?.toLowerCase() ?? "";
  const isMarkdown = ext === "md" || ext === "mdx" || ext === "markdown";

  return [
    nexusEditorTheme,
    lineNumbers(),
    highlightActiveLine(),
    drawSelection(),
    bracketMatching(),
    foldGutter(),
    indentOnInput(),
    history(),
    highlightSelectionMatches(),
    syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
    ...(isMarkdown ? [markdown(), liveMarkdown(), slashCommands()] : []),
    // Snippet Tab-expansion is listed before the default keymap so it runs
    // first; it returns false when no trigger matches, deferring to indent.
    snippetCompartment.of(currentSnippetExtension(filePath)),
    // Plugin-registered keybindings win over the CM6 defaults on exact
    // match because they're listed first in the keymap extension set.
    pluginKeymapCompartment.of(currentPluginKeymap()),
    keymap.of([
      ...defaultKeymap,
      ...historyKeymap,
      ...searchKeymap,
      {
        key: "Mod-s",
        run: (view) => {
          onSaveRef.current?.(view.state.doc.toString());
          return true;
        },
      },
    ]),
    pluginDecorationsCompartment.of(currentPluginDecorations()),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        onChangeRef.current?.(update.state.doc.toString());
      }
    }),
  ];
}

export function EditorSurface({
  initialContent,
  filePath,
  onChange,
  onSave,
}: EditorSurfaceProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  const onSaveRef = useRef(onSave);
  const initialContentRef = useRef(initialContent);
  const filePathRef = useRef(filePath);
  const pluginDecorationsCompartment = useMemo(() => new Compartment(), []);
  const pluginKeymapCompartment = useMemo(() => new Compartment(), []);
  const snippetCompartment = useMemo(() => new Compartment(), []);

  // Keep callback refs current without re-creating the editor.
  onChangeRef.current = onChange;
  onSaveRef.current = onSave;
  filePathRef.current = filePath;

  // Mount the editor on first render.
  useEffect(() => {
    if (!parentRef.current) return;

    const state = EditorState.create({
      doc: initialContentRef.current,
      extensions: getExtensions(
        filePath,
        onChangeRef,
        onSaveRef,
        pluginDecorationsCompartment,
        pluginKeymapCompartment,
        snippetCompartment,
      ),
    });
    const view = new EditorView({ state, parent: parentRef.current });
    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // filePath is intentionally excluded — handled by the content swap effect below
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // When the file changes (user opens a different file), swap the doc.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;

    // Only swap if content actually changed (avoid re-setting on first render).
    const currentDoc = view.state.doc.toString();
    if (currentDoc === initialContent) return;

    view.dispatch({
      changes: {
        from: 0,
        to: view.state.doc.length,
        insert: initialContent,
      },
    });
  }, [initialContent]);

  // Reconfigure the plugin-contribution compartments whenever the
  // decoration-provider or editor-keybinding registries change. A
  // hot-reloaded plugin drops its old entries and re-registers new
  // ones, and the editor picks that up without a remount.
  useEffect(() => {
    const resyncDecorations = () => {
      const view = viewRef.current;
      if (!view) return;
      view.dispatch({
        effects: pluginDecorationsCompartment.reconfigure(
          currentPluginDecorations(),
        ),
      });
    };
    const resyncKeymap = () => {
      const view = viewRef.current;
      if (!view) return;
      view.dispatch({
        effects: pluginKeymapCompartment.reconfigure(currentPluginKeymap()),
      });
    };
    const resyncSnippets = () => {
      const view = viewRef.current;
      if (!view) return;
      view.dispatch({
        effects: snippetCompartment.reconfigure(
          currentSnippetExtension(filePathRef.current),
        ),
      });
    };
    const offDec = contributions.subscribeEditorDecorationProviders(resyncDecorations);
    const offKey = contributions.subscribeEditorKeybindings(resyncKeymap);
    const offSnip = contributions.subscribeSnippets(resyncSnippets);
    return () => {
      offDec();
      offKey();
      offSnip();
    };
  }, [pluginDecorationsCompartment, pluginKeymapCompartment, snippetCompartment]);

  // Handle outline scroll-to-heading events.
  const handleScrollToHeading = useCallback((e: Event) => {
    const view = viewRef.current;
    if (!view) return;
    const line = (e as CustomEvent).detail?.line;
    if (typeof line !== "number") return;
    // CodeMirror lines are 1-indexed.
    const lineInfo = view.state.doc.line(Math.min(line, view.state.doc.lines));
    view.dispatch({
      effects: EditorView.scrollIntoView(lineInfo.from, { y: "start" }),
    });
  }, []);

  useEffect(() => {
    window.addEventListener("nx:scroll-to-heading", handleScrollToHeading);
    return () =>
      window.removeEventListener("nx:scroll-to-heading", handleScrollToHeading);
  }, [handleScrollToHeading]);

  return <div ref={parentRef} className="editor-surface" />;
}
