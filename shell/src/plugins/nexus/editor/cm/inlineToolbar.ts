// Phase 5 of docs/archive/notion-block-ux-plan.md — inline annotation
// toolbar + keyboard shortcuts.
//
// A small floating toolbar rides above any non-empty text selection
// and exposes the Notion-standard inline toggles: Bold, Italic,
// Code, Link. Each operation wraps (or unwraps) the selection with
// the appropriate markdown marker so the kernel reparse turns it
// into the equivalent `Annotation`. No new IPC.
//
// Keyboard shortcuts (`Mod-b` / `Mod-i` / `Mod-e` / `Mod-k`) hit the
// same wrap/unwrap path as the toolbar buttons, so they work even
// when the toolbar is off-screen (e.g. selection extends past the
// viewport).

import { EditorSelection, type Extension } from '@codemirror/state'
import {
  EditorView,
  ViewPlugin,
  keymap,
  type PluginValue,
  type ViewUpdate,
} from '@codemirror/view'
import { getEditorRuntime } from '../runtime'

// ── Wrap / unwrap helpers ────────────────────────────────────────────────────

type MarkerPair = { open: string; close: string }

const MARKERS: Record<string, MarkerPair> = {
  bold: { open: '**', close: '**' },
  italic: { open: '*', close: '*' },
  code: { open: '`', close: '`' },
}

function toggleWrap(view: EditorView, kind: keyof typeof MARKERS): void {
  const sel = view.state.selection.main
  if (sel.empty) return
  const { open, close } = MARKERS[kind]
  const doc = view.state.doc
  const from = Math.min(sel.anchor, sel.head)
  const to = Math.max(sel.anchor, sel.head)
  const inner = doc.sliceString(from, to)
  // If the selection is already wrapped (inner starts and ends with
  // the marker), strip them. Otherwise wrap.
  if (inner.startsWith(open) && inner.endsWith(close) && inner.length >= open.length + close.length) {
    const stripped = inner.slice(open.length, inner.length - close.length)
    view.dispatch({
      changes: { from, to, insert: stripped },
      selection: EditorSelection.range(from, from + stripped.length),
      userEvent: 'input.annotation.toggle',
    })
    return
  }
  // Also handle the case where the markers are immediately outside
  // the selection: `**foo**` with `foo` selected → strip on toggle.
  const outer = doc.sliceString(Math.max(0, from - open.length), Math.min(doc.length, to + close.length))
  if (outer.startsWith(open) && outer.endsWith(close)) {
    const wrappedStart = from - open.length
    const wrappedEnd = to + close.length
    view.dispatch({
      changes: { from: wrappedStart, to: wrappedEnd, insert: inner },
      selection: EditorSelection.range(wrappedStart, wrappedStart + inner.length),
      userEvent: 'input.annotation.toggle',
    })
    return
  }
  const wrapped = open + inner + close
  view.dispatch({
    changes: { from, to, insert: wrapped },
    selection: EditorSelection.range(from + open.length, from + open.length + inner.length),
    userEvent: 'input.annotation.toggle',
  })
}

async function insertLink(view: EditorView): Promise<void> {
  const sel = view.state.selection.main
  if (sel.empty) return
  const from = Math.min(sel.anchor, sel.head)
  const to = Math.max(sel.anchor, sel.head)
  const inner = view.state.doc.sliceString(from, to)
  // #202 / R12 — route through the host dialog surface (api.input.prompt)
  // so the call works inside the null-origin iframe sandbox where
  // `window.prompt` is disabled. Absent in test drivers: skip.
  const promptForLinkUrl = getEditorRuntime()?.promptForLinkUrl
  if (!promptForLinkUrl) return
  const url = await promptForLinkUrl()
  if (!url) return
  // Re-resolve the selection after the await — the user may have
  // clicked elsewhere while the prompt was open.
  const cur = view.state.selection.main
  const innerNow = view.state.doc.sliceString(
    Math.min(cur.anchor, cur.head),
    Math.max(cur.anchor, cur.head),
  )
  const targetFrom = cur.empty ? from : Math.min(cur.anchor, cur.head)
  const targetTo = cur.empty ? to : Math.max(cur.anchor, cur.head)
  const text = cur.empty ? inner : innerNow
  const replacement = `[${text}](${url})`
  view.dispatch({
    changes: { from: targetFrom, to: targetTo, insert: replacement },
    selection: EditorSelection.range(targetFrom, targetFrom + replacement.length),
    userEvent: 'input.annotation.link',
  })
}

// ── Floating toolbar ─────────────────────────────────────────────────────────

class InlineToolbarPlugin implements PluginValue {
  private readonly dom: HTMLDivElement
  private readonly view: EditorView

  constructor(view: EditorView) {
    this.view = view
    this.dom = document.createElement('div')
    this.dom.className = 'cm-inline-toolbar'
    this.dom.style.display = 'none'
    this.dom.style.position = 'absolute'
    this.dom.style.zIndex = '65'
    this.dom.addEventListener('mousedown', (e) => e.preventDefault())
    view.dom.appendChild(this.dom)
    this.renderButtons()
    this.sync()
  }

  update(u: ViewUpdate): void {
    if (!u.selectionSet && !u.docChanged && !u.viewportChanged && !u.geometryChanged) return

    // State-only checks: always safe inside update().
    const sel = this.view.state.selection.main
    if (sel.empty) {
      this.dom.style.display = 'none'
      return
    }
    const doc = this.view.state.doc
    const from = Math.min(sel.anchor, sel.head)
    const to   = Math.max(sel.anchor, sel.head)
    const fromLine = doc.lineAt(from).number
    const toLine   = doc.lineAt(to).number
    let blankBetween = false
    for (let i = fromLine; i <= toLine; i++) {
      if (doc.line(i).text.trim() === '') { blankBetween = true; break }
    }
    if (blankBetween) {
      this.dom.style.display = 'none'
      return
    }

    // Layout reads are forbidden inside update() — schedule via requestMeasure.
    this.view.requestMeasure({
      read: (view) => ({
        start:    view.coordsAtPos(from),
        end:      view.coordsAtPos(to),
        hostRect: view.dom.getBoundingClientRect(),
      }),
      write: ({ start, end, hostRect }) => {
        if (!start) { this.dom.style.display = 'none'; return }
        this.dom.style.display = 'flex'
        const selWidth    = Math.max(0, (end?.right ?? start.right) - start.left)
        const toolbarWidth = this.dom.offsetWidth || 160
        const centerX = start.left + selWidth / 2 - toolbarWidth / 2 - hostRect.left
        const left    = Math.max(4, Math.min(centerX, hostRect.width - toolbarWidth - 4))
        const aboveTop = start.top - hostRect.top - 36
        const belowTop = (end?.bottom ?? start.bottom) - hostRect.top + 4
        this.dom.style.left = `${left}px`
        this.dom.style.top  = `${aboveTop < 4 ? belowTop : aboveTop}px`
      },
    })
  }

  destroy(): void {
    this.dom.remove()
  }

  private renderButtons(): void {
    const items: Array<{ label: string; title: string; action: () => void }> = [
      { label: 'B', title: 'Bold (Cmd/Ctrl+B)', action: () => toggleWrap(this.view, 'bold') },
      { label: 'I', title: 'Italic (Cmd/Ctrl+I)', action: () => toggleWrap(this.view, 'italic') },
      { label: '⌨', title: 'Code (Cmd/Ctrl+E)', action: () => toggleWrap(this.view, 'code') },
      { label: '🔗', title: 'Link (Cmd/Ctrl+K)', action: () => { void insertLink(this.view) } },
    ]
    this.dom.textContent = ''
    for (const it of items) {
      const btn = document.createElement('button')
      btn.type = 'button'
      btn.className = 'cm-inline-toolbar__btn'
      btn.textContent = it.label
      btn.title = it.title
      btn.addEventListener('click', (e) => {
        e.preventDefault()
        e.stopPropagation()
        it.action()
        // Keep focus in CM so the selection stays alive for follow-ups.
        this.view.focus()
      })
      this.dom.appendChild(btn)
    }
  }

  private sync(): void {
    const sel = this.view.state.selection.main
    if (sel.empty) {
      this.dom.style.display = 'none'
      return
    }
    // Hide for multi-line selections that span a blank line — those
    // are block-mode selections (Phase 2) and aren't the target here.
    const doc = this.view.state.doc
    const from = Math.min(sel.anchor, sel.head)
    const to = Math.max(sel.anchor, sel.head)
    const fromLine = doc.lineAt(from).number
    const toLine = doc.lineAt(to).number
    let blankBetween = false
    for (let i = fromLine; i <= toLine; i++) {
      if (doc.line(i).text.trim() === '') {
        blankBetween = true
        break
      }
    }
    if (blankBetween) {
      this.dom.style.display = 'none'
      return
    }
    const start = this.view.coordsAtPos(from)
    if (!start) {
      this.dom.style.display = 'none'
      return
    }
    const hostRect = this.view.dom.getBoundingClientRect()
    // Center the toolbar horizontally over the selection's starting
    // coord, clamp to viewport, and anchor above the selection with a
    // fallback below if there's no room.
    this.dom.style.display = 'flex'
    const selWidth = Math.max(
      0,
      (this.view.coordsAtPos(to)?.right ?? start.right) - start.left,
    )
    const toolbarWidth = this.dom.offsetWidth || 160
    const centerX = start.left + selWidth / 2 - toolbarWidth / 2 - hostRect.left
    const left = Math.max(4, Math.min(centerX, hostRect.width - toolbarWidth - 4))
    const aboveTop = start.top - hostRect.top - 36
    const belowTop = (this.view.coordsAtPos(to)?.bottom ?? start.bottom) - hostRect.top + 4
    this.dom.style.left = `${left}px`
    this.dom.style.top = `${aboveTop < 4 ? belowTop : aboveTop}px`
  }
}

// ── Public extension ────────────────────────────────────────────────────────

export function inlineToolbarExt(): Extension {
  return [
    ViewPlugin.fromClass(InlineToolbarPlugin),
    keymap.of([
      { key: 'Mod-b', run: (view) => { toggleWrap(view, 'bold'); return true } },
      { key: 'Mod-i', run: (view) => { toggleWrap(view, 'italic'); return true } },
      { key: 'Mod-e', run: (view) => { toggleWrap(view, 'code'); return true } },
      { key: 'Mod-k', run: (view) => { void insertLink(view); return true } },
    ]),
  ]
}

/** Styles for the floating toolbar. Called once by the editor plugin
 *  `activate`. */
export function installInlineToolbarStyles(): () => void {
  const id = 'nexus-editor-inline-toolbar-styles'
  if (document.getElementById(id)) return () => undefined
  const style = document.createElement('style')
  style.id = id
  style.textContent = `
.cm-inline-toolbar {
  display: flex;
  gap: 2px;
  padding: 3px;
  background: var(--background-secondary);
  border: 1px solid var(--divider-color);
  border-radius: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
  font-family: var(--font-family, system-ui, sans-serif);
  font-size: 12px;
}
.cm-inline-toolbar__btn {
  min-width: 28px;
  height: 26px;
  padding: 0 8px;
  background: transparent;
  color: var(--text-normal);
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font: inherit;
  font-weight: 500;
}
.cm-inline-toolbar__btn:hover {
  background: var(--background-modifier-hover);
}
.cm-inline-toolbar__btn:active {
  background: var(--background-modifier-active);
}
`
  document.head.appendChild(style)
  return () => style.remove()
}
