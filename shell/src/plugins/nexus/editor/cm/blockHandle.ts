// Phase 3 of docs/notion-block-ux-plan.md — block handle + per-block
// menu + drag reorder.
//
// A 6-dot grip glyph renders in the left gutter next to each block's
// first line. Hovering over a block's rows fades it in; clicking
// opens a dropdown with the standard block operations (Turn into,
// Duplicate, Delete, Move up, Move down). Dragging the handle
// reorders the block by cutting its text and inserting it at the
// target position — a drop-line indicator follows the cursor so the
// target is obvious.
//
// All mutations go through plain CM `dispatch({ changes })`. The
// kernel's `editor_sync_content` debounce picks up the edit and the
// Rust block tree reconverges on the new shape — no new IPC.

import { EditorSelection, StateEffect, StateField, type Extension } from '@codemirror/state'
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type PluginValue,
  type ViewUpdate,
} from '@codemirror/view'

// ── Block range helpers (shared semantics with blockSelection.ts) ────────────

export interface BlockRange {
  /** Line number (1-based) of the block's first line. */
  startLine: number
  /** Line number of the block's last line. */
  endLine: number
  /** Document offset of the first character in the block. */
  from: number
  /** Document offset one past the last character (before the closing
   *  newline). */
  to: number
}

/** Compute every block in the current document. A block is a maximal
 *  run of consecutive non-blank lines. Blank separator lines belong
 *  to no block. Linear in the number of lines. */
export function scanBlocks(view: EditorView): BlockRange[] {
  const doc = view.state.doc
  const out: BlockRange[] = []
  let start = -1
  for (let i = 1; i <= doc.lines; i++) {
    const line = doc.line(i)
    const blank = line.text.trim() === ''
    if (!blank && start < 0) start = i
    if ((blank || i === doc.lines) && start >= 0) {
      const endLine = blank ? i - 1 : i
      out.push({
        startLine: start,
        endLine,
        from: doc.line(start).from,
        to: doc.line(endLine).to,
      })
      start = -1
    }
  }
  return out
}

export function blockAtLine(blocks: BlockRange[], line: number): BlockRange | null {
  for (const b of blocks) {
    if (line >= b.startLine && line <= b.endLine) return b
  }
  return null
}

// ── Transformations ──────────────────────────────────────────────────────────

function stripBlockPrefix(line: string): string {
  return line
    .replace(/^(#+\s+)/, '')
    .replace(/^(-\s+\[[ xX]\]\s+)/, '')
    .replace(/^(-\s+)/, '')
    .replace(/^(\d+\.\s+)/, '')
    .replace(/^(>\s?)/, '')
    .trim()
}

interface BlockTransform {
  id: string
  label: string
  rewrite: (first: string) => string
}

const BLOCK_TRANSFORMS: BlockTransform[] = [
  { id: 'text', label: 'Text', rewrite: (l) => stripBlockPrefix(l) },
  { id: 'h1', label: 'Heading 1', rewrite: (l) => `# ${stripBlockPrefix(l)}` },
  { id: 'h2', label: 'Heading 2', rewrite: (l) => `## ${stripBlockPrefix(l)}` },
  { id: 'h3', label: 'Heading 3', rewrite: (l) => `### ${stripBlockPrefix(l)}` },
  { id: 'bullet', label: 'Bullet list', rewrite: (l) => `- ${stripBlockPrefix(l)}` },
  { id: 'numbered', label: 'Numbered list', rewrite: (l) => `1. ${stripBlockPrefix(l)}` },
  { id: 'todo', label: 'To-do', rewrite: (l) => `- [ ] ${stripBlockPrefix(l)}` },
  { id: 'quote', label: 'Quote', rewrite: (l) => `> ${stripBlockPrefix(l)}` },
]

function transformBlock(view: EditorView, block: BlockRange, transform: BlockTransform): void {
  const doc = view.state.doc
  const firstLine = doc.line(block.startLine)
  const restStart = block.startLine < block.endLine ? doc.line(block.startLine + 1).from : -1
  const firstReplacement = transform.rewrite(firstLine.text)
  const rest = restStart >= 0 ? doc.sliceString(restStart, block.to) : ''
  const insert = rest ? `${firstReplacement}\n${rest}` : firstReplacement
  view.dispatch({
    changes: { from: block.from, to: block.to, insert },
    userEvent: 'input.block-transform',
  })
}

function duplicateBlock(view: EditorView, block: BlockRange): void {
  const text = view.state.doc.sliceString(block.from, block.to)
  view.dispatch({
    changes: { from: block.to, to: block.to, insert: `\n\n${text}` },
    userEvent: 'input.block-duplicate',
  })
}

function deleteBlock(view: EditorView, block: BlockRange): void {
  // Also consume the trailing newline(s) up to the next non-blank
  // line so we don't leave an unexpected empty line behind.
  const doc = view.state.doc
  let to = block.to
  while (to < doc.length && /[\n\r]/.test(doc.sliceString(to, to + 1))) to++
  view.dispatch({
    changes: { from: block.from, to, insert: '' },
    selection: EditorSelection.cursor(block.from),
    userEvent: 'delete.block',
  })
}

function moveBlock(view: EditorView, block: BlockRange, direction: 'up' | 'down'): void {
  const all = scanBlocks(view)
  const idx = all.findIndex((b) => b.from === block.from && b.to === block.to)
  if (idx < 0) return
  const target = direction === 'up' ? all[idx - 1] : all[idx + 1]
  if (!target) return
  reorderBlock(view, block, target, direction === 'down' ? 'after' : 'before')
}

/** Move `source` to land `side` of `target`. Preserves blank-line
 *  spacing by rewriting the full span between the outer brackets. */
function reorderBlock(
  view: EditorView,
  source: BlockRange,
  target: BlockRange,
  side: 'before' | 'after',
): void {
  const doc = view.state.doc
  if (source.from === target.from) return
  const sourceText = doc.sliceString(source.from, source.to)

  // Work in line numbers — splice the source block out of the list,
  // then rebuild the rest joined by blank lines. The current cursor
  // lands at the start of the moved block post-splice.
  const all = scanBlocks(view)
  const idx = all.findIndex((b) => b.from === source.from && b.to === source.to)
  const targetIdx = all.findIndex((b) => b.from === target.from && b.to === target.to)
  if (idx < 0 || targetIdx < 0) return

  const working = [...all]
  const [removed] = working.splice(idx, 1)
  // Target index may shift after removal.
  let insertAt = working.findIndex((b) => b.from === target.from && b.to === target.to)
  if (side === 'after') insertAt += 1
  working.splice(insertAt, 0, removed)

  const parts = working.map((b) => doc.sliceString(b.from, b.to))
  const rebuilt = parts.join('\n\n')
  const leadingBlank = doc.line(1).text.trim() === '' ? '' : '' // noop, here for clarity

  // Replace the document span from the first non-blank line to the
  // last non-blank line with the reordered blocks, keeping any
  // leading/trailing blank span intact.
  const first = all[0]
  const last = all[all.length - 1]
  const before = doc.sliceString(0, first.from)
  const after = doc.sliceString(last.to, doc.length)
  // Find the new offset of the source block in the rebuilt string so
  // we can place the caret there.
  const rebuiltIdx = working.findIndex((b) => b === removed)
  const caretOffset =
    first.from + parts.slice(0, rebuiltIdx).reduce((acc, p) => acc + p.length + 2, 0)

  view.dispatch({
    changes: { from: 0, to: doc.length, insert: before + rebuilt + after },
    selection: EditorSelection.cursor(caretOffset),
    userEvent: 'move.block',
  })
  // Silence unused-var when leadingBlank is dropped by the bundler.
  void leadingBlank
}

// ── State effects + field ────────────────────────────────────────────────────

interface MenuState {
  /** First-line doc offset of the block the menu is open for. */
  anchorPos: number
  /** Pixel position of the handle that anchors the menu. */
  x: number
  y: number
  /** Currently-open "Turn into" submenu flag. */
  turnIntoOpen: boolean
}

const openMenu = StateEffect.define<MenuState>()
const closeMenu = StateEffect.define<void>()
const setTurnIntoOpen = StateEffect.define<boolean>()

const menuField = StateField.define<MenuState | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(openMenu)) return e.value
      if (e.is(closeMenu)) return null
      if (e.is(setTurnIntoOpen) && value) return { ...value, turnIntoOpen: e.value }
    }
    return value
  },
})

// ── Drag state ───────────────────────────────────────────────────────────────

interface DragState {
  block: BlockRange
  dropLine: number
  side: 'before' | 'after'
}

const startDrag = StateEffect.define<DragState>()
const updateDrag = StateEffect.define<{ dropLine: number; side: 'before' | 'after' }>()
const endDrag = StateEffect.define<void>()

const dragField = StateField.define<DragState | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(startDrag)) return e.value
      if (e.is(endDrag)) return null
      if (e.is(updateDrag) && value) return { ...value, ...e.value }
    }
    return value
  },
})

// Handle DOM is rendered by the ViewPlugin below — an absolutely-
// positioned element per block, y-tracked to the block's first line.
// An explicit ViewPlugin keeps us clear of the CM `gutter` API's
// stricter typing (GutterMarker lifecycle) and lets us hit-test the
// handle element directly in mousedown/click.

// ── ViewPlugin: menu + drag DOM ──────────────────────────────────────────────

class BlockHandlePlugin implements PluginValue {
  private readonly view: EditorView
  private readonly menu: HTMLDivElement
  private readonly dropLine: HTMLDivElement
  private readonly handlesLayer: HTMLDivElement
  private dragging: { block: BlockRange } | null = null

  constructor(view: EditorView) {
    this.view = view
    // Relative positioning on the host so absolute children (handle
    // layer, menu, drop line) pin to the editor rect.
    if (getComputedStyle(view.dom).position === 'static') {
      view.dom.style.position = 'relative'
    }

    this.handlesLayer = document.createElement('div')
    this.handlesLayer.className = 'cm-block-handles-layer'
    this.handlesLayer.style.position = 'absolute'
    this.handlesLayer.style.left = '0'
    this.handlesLayer.style.top = '0'
    this.handlesLayer.style.width = '22px'
    this.handlesLayer.style.bottom = '0'
    this.handlesLayer.style.pointerEvents = 'none'
    this.handlesLayer.style.zIndex = '60'
    view.dom.appendChild(this.handlesLayer)

    this.menu = document.createElement('div')
    this.menu.className = 'cm-block-menu'
    this.menu.style.display = 'none'
    this.menu.style.position = 'absolute'
    this.menu.style.zIndex = '70'
    this.menu.addEventListener('mousedown', (e) => e.preventDefault())
    view.dom.appendChild(this.menu)

    this.dropLine = document.createElement('div')
    this.dropLine.className = 'cm-block-drop-line'
    this.dropLine.style.display = 'none'
    this.dropLine.style.position = 'absolute'
    this.dropLine.style.pointerEvents = 'none'
    this.dropLine.style.height = '2px'
    view.dom.appendChild(this.dropLine)

    view.dom.addEventListener('click', this.onClick)
    view.dom.addEventListener('mousedown', this.onMouseDown)
    window.addEventListener('mousemove', this.onMouseMove)
    window.addEventListener('mouseup', this.onMouseUp)
    document.addEventListener('mousedown', this.onGlobalMouseDown)
    this.sync()
  }

  update(u: ViewUpdate): void {
    if (u.docChanged || u.viewportChanged || u.transactions.length > 0) this.sync()
  }

  destroy(): void {
    this.view.dom.removeEventListener('click', this.onClick)
    this.view.dom.removeEventListener('mousedown', this.onMouseDown)
    window.removeEventListener('mousemove', this.onMouseMove)
    window.removeEventListener('mouseup', this.onMouseUp)
    document.removeEventListener('mousedown', this.onGlobalMouseDown)
    this.handlesLayer.remove()
    this.menu.remove()
    this.dropLine.remove()
  }

  private findHandle(event: Event): HTMLElement | null {
    const target = event.target as HTMLElement | null
    if (!target) return null
    return target.closest('.cm-block-handle') as HTMLElement | null
  }

  private blockFromHandle(handle: HTMLElement): BlockRange | null {
    const from = Number(handle.dataset.blockFrom)
    if (!Number.isFinite(from)) return null
    return scanBlocks(this.view).find((b) => b.from === from) ?? null
  }

  private onClick = (e: MouseEvent): void => {
    const handle = this.findHandle(e)
    if (!handle || this.dragging) return
    if ((e as MouseEvent & { _handledByDrag?: boolean })._handledByDrag) return
    const block = this.blockFromHandle(handle)
    if (!block) return
    e.preventDefault()
    e.stopPropagation()
    const rect = handle.getBoundingClientRect()
    const hostRect = this.view.dom.getBoundingClientRect()
    this.view.dispatch({
      effects: openMenu.of({
        anchorPos: block.from,
        x: rect.left - hostRect.left + rect.width + 4,
        y: rect.top - hostRect.top,
        turnIntoOpen: false,
      }),
    })
  }

  private onMouseDown = (e: MouseEvent): void => {
    if (e.button !== 0) return
    const handle = this.findHandle(e)
    if (!handle) return
    const block = this.blockFromHandle(handle)
    if (!block) return
    // Start a tentative drag — if the user releases without moving we
    // treat it as a click and fall through to onClick.
    this.dragging = { block }
  }

  private onMouseMove = (e: MouseEvent): void => {
    if (!this.dragging) return
    const rect = this.view.dom.getBoundingClientRect()
    const pos = this.view.posAtCoords({ x: e.clientX, y: e.clientY })
    if (pos == null) return
    const lineNo = this.view.state.doc.lineAt(pos).number
    const blocks = scanBlocks(this.view)
    const target = blockAtLine(blocks, lineNo)
    if (!target || target.from === this.dragging.block.from) {
      this.dropLine.style.display = 'none'
      return
    }
    // Drop either above or below based on which half of the target
    // block the pointer is in.
    const mid = (this.view.coordsAtPos(target.from)?.top ?? 0) +
      (this.view.coordsAtPos(target.to)?.bottom ?? 0)
    const halfY = mid / 2
    const side: 'before' | 'after' = e.clientY < halfY ? 'before' : 'after'
    const anchor = this.view.coordsAtPos(side === 'before' ? target.from : target.to)
    if (!anchor) return
    this.dropLine.style.display = 'block'
    this.dropLine.style.left = `0px`
    this.dropLine.style.right = `0px`
    this.dropLine.style.width = `${rect.width}px`
    this.dropLine.style.top = `${(side === 'before' ? anchor.top : anchor.bottom) - rect.top - 1}px`
  }

  private onMouseUp = (e: MouseEvent): void => {
    if (!this.dragging) return
    const drag = this.dragging
    this.dragging = null
    this.dropLine.style.display = 'none'
    const pos = this.view.posAtCoords({ x: e.clientX, y: e.clientY })
    if (pos == null) return
    const lineNo = this.view.state.doc.lineAt(pos).number
    const blocks = scanBlocks(this.view)
    const target = blockAtLine(blocks, lineNo)
    if (!target || target.from === drag.block.from) return
    const targetCoords = this.view.coordsAtPos(target.from)
    const midY =
      ((this.view.coordsAtPos(target.from)?.top ?? 0) +
        (this.view.coordsAtPos(target.to)?.bottom ?? 0)) / 2
    const side: 'before' | 'after' = e.clientY < midY ? 'before' : 'after'
    // Flag the click event that follows so onClick doesn't open a menu.
    ;(e as MouseEvent & { _handledByDrag?: boolean })._handledByDrag = true
    void targetCoords
    reorderBlock(this.view, drag.block, target, side)
  }

  private onGlobalMouseDown = (e: MouseEvent): void => {
    if (!this.view.state.field(menuField)) return
    const target = e.target as Node | null
    if (this.menu.contains(target)) return
    if (target instanceof HTMLElement && target.closest('.cm-block-handle')) return
    this.view.dispatch({ effects: closeMenu.of() })
  }

  sync(): void {
    this.renderHandles()
    const state = this.view.state.field(menuField)
    if (!state) {
      this.menu.style.display = 'none'
      this.menu.textContent = ''
      return
    }
    const blocks = scanBlocks(this.view)
    const block = blocks.find((b) => b.from === state.anchorPos)
    if (!block) {
      this.view.dispatch({ effects: closeMenu.of() })
      return
    }
    this.menu.style.display = 'block'
    this.menu.style.left = `${state.x}px`
    this.menu.style.top = `${state.y}px`
    this.menu.textContent = ''
    this.renderMenuItems(block, state.turnIntoOpen)
  }

  private renderHandles(): void {
    // Rebuild each sync — the block count is small (tens, not
    // thousands) and sync only runs on doc / viewport change.
    this.handlesLayer.textContent = ''
    const hostRect = this.view.dom.getBoundingClientRect()
    const blocks = scanBlocks(this.view)
    for (const b of blocks) {
      const coords = this.view.coordsAtPos(b.from)
      if (!coords) continue
      const top = coords.top - hostRect.top
      const handle = document.createElement('div')
      handle.className = 'cm-block-handle'
      handle.dataset.blockFrom = String(b.from)
      handle.title = 'Block options · drag to reorder'
      handle.setAttribute('aria-label', 'Block handle')
      handle.style.position = 'absolute'
      handle.style.left = '2px'
      handle.style.top = `${top + 2}px`
      handle.style.pointerEvents = 'auto'
      handle.innerHTML = '<span class="cm-block-handle__dot"></span>'.repeat(6)
      this.handlesLayer.appendChild(handle)
    }
  }

  private renderMenuItems(block: BlockRange, turnIntoOpen: boolean): void {
    const items: Array<{ label: string; action: () => void; hasSubmenu?: boolean }> = [
      {
        label: 'Turn into ▸',
        action: () => this.view.dispatch({ effects: setTurnIntoOpen.of(!turnIntoOpen) }),
        hasSubmenu: true,
      },
      {
        label: 'Duplicate',
        action: () => {
          duplicateBlock(this.view, block)
          this.view.dispatch({ effects: closeMenu.of() })
        },
      },
      {
        label: 'Move up',
        action: () => {
          moveBlock(this.view, block, 'up')
          this.view.dispatch({ effects: closeMenu.of() })
        },
      },
      {
        label: 'Move down',
        action: () => {
          moveBlock(this.view, block, 'down')
          this.view.dispatch({ effects: closeMenu.of() })
        },
      },
      {
        label: 'Delete',
        action: () => {
          deleteBlock(this.view, block)
          this.view.dispatch({ effects: closeMenu.of() })
        },
      },
    ]
    for (const it of items) {
      const row = document.createElement('div')
      row.className = 'cm-block-menu__row'
      row.textContent = it.label
      row.addEventListener('click', (e) => {
        e.preventDefault()
        e.stopPropagation()
        it.action()
      })
      this.menu.appendChild(row)
    }
    if (turnIntoOpen) {
      const submenu = document.createElement('div')
      submenu.className = 'cm-block-menu__submenu'
      for (const t of BLOCK_TRANSFORMS) {
        const row = document.createElement('div')
        row.className = 'cm-block-menu__row'
        row.textContent = t.label
        row.addEventListener('click', (e) => {
          e.preventDefault()
          e.stopPropagation()
          transformBlock(this.view, block, t)
          this.view.dispatch({ effects: closeMenu.of() })
        })
        submenu.appendChild(row)
      }
      this.menu.appendChild(submenu)
    }
  }
}

// ── Decorations (reserved for future hover highlight) ───────────────────────

function buildDecorations(): DecorationSet {
  return Decoration.none
}

// ── Public extension ────────────────────────────────────────────────────────

export function blockHandleExt(): Extension {
  const plugin = ViewPlugin.fromClass(BlockHandlePlugin)
  return [
    menuField,
    dragField,
    plugin,
    EditorView.decorations.compute([menuField, dragField], () => buildDecorations()),
  ]
}

/** Styles for the handle, menu, and drop line. Injected once by the
 *  editor plugin's `activate`. */
export function installBlockHandleStyles(): () => void {
  const id = 'nexus-editor-block-handle-styles'
  if (document.getElementById(id)) return () => undefined
  const style = document.createElement('style')
  style.id = id
  style.textContent = `
.cm-block-handles-layer {
  /* Width of the overlay column; pushes handles left of the text. */
}
.cm-block-handle {
  display: grid;
  grid-template-columns: repeat(2, 3px);
  grid-auto-rows: 3px;
  gap: 2px;
  width: 12px;
  padding: 3px 2px;
  opacity: 0.25;
  cursor: grab;
  border-radius: 3px;
  transition: opacity 120ms ease, background 120ms ease;
}
.cm-block-handle__dot {
  width: 3px;
  height: 3px;
  background: var(--fg-muted, #9ca3af);
  border-radius: 50%;
}
.cm-block-handle:hover {
  opacity: 1;
  background: var(--bg-hover, #363636);
}
.cm-block-handle:active {
  cursor: grabbing;
}
.cm-editor {
  /* Leave room on the left so handles don't overlap the text. */
  padding-left: 22px;
}
.cm-block-menu {
  min-width: 200px;
  background: var(--bg-raised, #2d2d2d);
  color: var(--fg, #e5e7eb);
  border: 1px solid var(--divider-color, #3f3f46);
  border-radius: 6px;
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.35);
  font-family: var(--font-family, system-ui, sans-serif);
  font-size: 12px;
  padding: 4px 0;
}
.cm-block-menu__row {
  padding: 6px 12px;
  cursor: pointer;
}
.cm-block-menu__row:hover {
  background: var(--bg-hover, #363636);
}
.cm-block-menu__submenu {
  border-top: 1px solid var(--divider-color, #3f3f46);
  margin-top: 4px;
  padding-top: 4px;
}
.cm-block-drop-line {
  background: var(--accent, #60a5fa);
  box-shadow: 0 0 4px var(--accent, #60a5fa);
  z-index: 65;
}
`
  document.head.appendChild(style)
  return () => style.remove()
}
