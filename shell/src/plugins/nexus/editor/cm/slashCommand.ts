// Phase 1 of docs/notion-block-ux-plan.md — slash command menu.
//
// Typing `/` at the start of a block (or after whitespace only) opens
// a small overlay anchored at the cursor. Each entry rewrites the
// current line's text — since the kernel re-parses the markdown
// through `editor_sync_content`, the block tree converges on the new
// BlockType automatically. No new Rust handler needed (per the plan).
//
// The overlay is a DOM element mounted as a CM `ViewPlugin`; its
// state lives in a CM `StateField` so effects drive it (open / filter
// update / close) rather than imperative ref surgery.

import {
  EditorSelection,
  StateEffect,
  StateField,
  type Transaction,
} from '@codemirror/state'
import {
  Decoration,
  EditorView,
  ViewPlugin,
  keymap,
  type DecorationSet,
  type PluginValue,
  type ViewUpdate,
} from '@codemirror/view'
import type { Extension } from '@codemirror/state'

// ── Command registry ─────────────────────────────────────────────────────────

export interface SlashCommand {
  id: string
  /** Human-readable label shown in the palette. */
  label: string
  /** Optional one-line description to the right of the label. */
  description?: string
  /** Optional category header for grouping. */
  category?: string
  /** Single-character icon/glyph rendered at the left of the row. */
  glyph?: string
  /** Rewrites the current line. Receives the plain line text (without
   *  the leading slash + query) and returns the replacement. A return
   *  value of `null` means "leave the line alone"; the menu still
   *  closes. */
  rewrite: (existingLine: string) => string | null
  /** Keywords to match against the query string in addition to label/id. */
  keywords?: string[]
}

const BUILTIN_COMMANDS: SlashCommand[] = [
  {
    id: 'text',
    label: 'Text',
    description: 'Plain paragraph',
    category: 'Basic',
    glyph: '¶',
    keywords: ['paragraph', 'plain'],
    rewrite: (existing) => stripBlockPrefix(existing),
  },
  {
    id: 'h1',
    label: 'Heading 1',
    description: 'Top-level section',
    category: 'Basic',
    glyph: 'H₁',
    keywords: ['h1', 'heading'],
    rewrite: (existing) => `# ${stripBlockPrefix(existing)}`,
  },
  {
    id: 'h2',
    label: 'Heading 2',
    category: 'Basic',
    glyph: 'H₂',
    keywords: ['h2', 'heading'],
    rewrite: (existing) => `## ${stripBlockPrefix(existing)}`,
  },
  {
    id: 'h3',
    label: 'Heading 3',
    category: 'Basic',
    glyph: 'H₃',
    keywords: ['h3', 'heading'],
    rewrite: (existing) => `### ${stripBlockPrefix(existing)}`,
  },
  {
    id: 'bullet',
    label: 'Bullet list',
    description: 'Unordered list',
    category: 'Lists',
    glyph: '•',
    keywords: ['ul', 'unordered'],
    rewrite: (existing) => `- ${stripBlockPrefix(existing)}`,
  },
  {
    id: 'numbered',
    label: 'Numbered list',
    description: 'Ordered list',
    category: 'Lists',
    glyph: '1.',
    keywords: ['ol', 'ordered'],
    rewrite: (existing) => `1. ${stripBlockPrefix(existing)}`,
  },
  {
    id: 'todo',
    label: 'To-do',
    description: 'Checkbox list',
    category: 'Lists',
    glyph: '☐',
    keywords: ['checkbox', 'task'],
    rewrite: (existing) => `- [ ] ${stripBlockPrefix(existing)}`,
  },
  {
    id: 'quote',
    label: 'Quote',
    category: 'Blocks',
    glyph: '❝',
    keywords: ['blockquote'],
    rewrite: (existing) => `> ${stripBlockPrefix(existing)}`,
  },
  {
    id: 'callout',
    label: 'Callout',
    description: 'Obsidian-style admonition',
    category: 'Blocks',
    glyph: '❗',
    keywords: ['admonition', 'note'],
    rewrite: (existing) => `> [!note] ${stripBlockPrefix(existing)}`,
  },
  {
    id: 'code',
    label: 'Code block',
    description: 'Fenced code',
    category: 'Blocks',
    glyph: '</>',
    keywords: ['snippet', 'fence'],
    rewrite: (existing) => '```\n' + stripBlockPrefix(existing) + '\n```',
  },
  {
    id: 'divider',
    label: 'Divider',
    description: 'Horizontal rule',
    category: 'Blocks',
    glyph: '—',
    keywords: ['hr', 'rule', 'separator'],
    rewrite: () => '---',
  },
  {
    id: 'math',
    label: 'Math block',
    description: 'Display-mode LaTeX',
    category: 'Blocks',
    glyph: '∑',
    keywords: ['latex', 'formula'],
    rewrite: (existing) => '$$\n' + stripBlockPrefix(existing) + '\n$$',
  },
]

/** Registry lives outside the extension so plugins can add/remove
 *  commands at runtime. The slash palette consults it on every open. */
class SlashCommandRegistry {
  private readonly commands = new Map<string, SlashCommand>()

  constructor(seed: SlashCommand[]) {
    for (const c of seed) this.commands.set(c.id, c)
  }

  register(command: SlashCommand): () => void {
    this.commands.set(command.id, command)
    return () => {
      if (this.commands.get(command.id) === command) {
        this.commands.delete(command.id)
      }
    }
  }

  all(): SlashCommand[] {
    return Array.from(this.commands.values())
  }
}

export const slashCommands = new SlashCommandRegistry(BUILTIN_COMMANDS)

/** Strip a markdown block-prefix (heading hashes, list bullets, quote
 *  angle, checkbox) from the start of a line so rewrites swap prefix
 *  cleanly instead of stacking. */
function stripBlockPrefix(line: string): string {
  return line
    .replace(/^(#+\s+)/, '')
    .replace(/^(-\s+\[[ xX]\]\s+)/, '')
    .replace(/^(-\s+)/, '')
    .replace(/^(\d+\.\s+)/, '')
    .replace(/^(>\s?)/, '')
    .trim()
}

// ── Menu state ───────────────────────────────────────────────────────────────

interface MenuState {
  /** Document offset of the `/` that opened the menu. */
  from: number
  /** Document offset of the cursor while the menu is open. */
  to: number
  /** Text typed after the `/` — drives the filter. */
  query: string
  /** Highlighted command index (0-based into the filtered list). */
  highlight: number
}

const openMenu = StateEffect.define<{ from: number }>()
const closeMenu = StateEffect.define<void>()
const updateMenu = StateEffect.define<Partial<MenuState>>()

const menuField = StateField.define<MenuState | null>({
  create: () => null,
  update(value, tr) {
    let next = value
    for (const e of tr.effects) {
      if (e.is(openMenu)) {
        next = { from: e.value.from, to: e.value.from + 1, query: '', highlight: 0 }
      } else if (e.is(closeMenu)) {
        next = null
      } else if (e.is(updateMenu)) {
        if (next) next = { ...next, ...e.value }
      }
    }
    if (next && tr.docChanged) {
      // Follow the user's typing; close if the user deleted the slash
      // or moved the cursor backwards past it.
      const head = tr.state.selection.main.head
      const slashChar = tr.state.doc.sliceString(next.from, next.from + 1)
      if (slashChar !== '/' || head < next.from + 1) {
        next = null
      } else {
        const query = tr.state.doc.sliceString(next.from + 1, head)
        // Close if the user typed a newline or spaces (same as typing
        // past the palette).
        if (/\n/.test(query)) {
          next = null
        } else {
          next = { ...next, to: head, query, highlight: 0 }
        }
      }
    }
    return next
  },
})

function canOpenAt(state: { doc: { lineAt(pos: number): { from: number; text: string } } }, pos: number): boolean {
  // Allow at block start or when everything between the line start and
  // `pos` is whitespace. Mirrors Notion / Obsidian.
  const line = state.doc.lineAt(pos)
  const prefix = line.text.slice(0, pos - line.from)
  return /^\s*$/.test(prefix)
}

/** Find the best matches in `commands` against `query`. Empty query
 *  returns everything in declaration order. Otherwise: lowercase
 *  substring match over id / label / keywords. A simple sort by match
 *  rank (start-of-word wins over middle). */
function filterCommands(commands: SlashCommand[], query: string): SlashCommand[] {
  if (!query) return commands
  const q = query.toLowerCase()
  const hits: Array<{ cmd: SlashCommand; rank: number }> = []
  for (const cmd of commands) {
    const candidates = [cmd.id, cmd.label, ...(cmd.keywords ?? [])]
    let best = Infinity
    for (const c of candidates) {
      const idx = c.toLowerCase().indexOf(q)
      if (idx < 0) continue
      // Start-of-word hits rank ahead of mid-string hits.
      const rank = idx === 0 ? 0 : idx + 1
      if (rank < best) best = rank
    }
    if (best !== Infinity) hits.push({ cmd, rank: best })
  }
  hits.sort((a, b) => a.rank - b.rank)
  return hits.map((h) => h.cmd)
}

function applyCommand(view: EditorView, menu: MenuState, cmd: SlashCommand): void {
  const line = view.state.doc.lineAt(menu.from)
  const existing = line.text
  // The existing line, minus the slash-query segment.
  const before = existing.slice(0, menu.from - line.from)
  const after = existing.slice(menu.to - line.from)
  const lineMinusSlash = (before + after).trim()
  const replacement = cmd.rewrite(lineMinusSlash)
  if (replacement === null) {
    view.dispatch({ effects: closeMenu.of() })
    return
  }
  // Place cursor after the last non-whitespace character of the
  // replacement's first line so the user can keep typing.
  const firstNewline = replacement.indexOf('\n')
  const caret = line.from + (firstNewline >= 0 ? firstNewline : replacement.length)
  view.dispatch({
    changes: { from: line.from, to: line.to, insert: replacement },
    selection: EditorSelection.cursor(caret),
    effects: closeMenu.of(),
    userEvent: 'input.slash-command',
  })
}

// ── Decoration: subtle underline on the in-flight slash+query ────────────────

const slashMark = Decoration.mark({ class: 'cm-slash-query' })

function buildDecorations(menu: MenuState | null): DecorationSet {
  if (!menu || menu.to <= menu.from) return Decoration.none
  return Decoration.set([slashMark.range(menu.from, menu.to)])
}

// ── ViewPlugin: DOM overlay + key handling ───────────────────────────────────

class SlashMenuPlugin implements PluginValue {
  private readonly dom: HTMLDivElement
  private readonly view: EditorView
  private lastMatches: SlashCommand[] = []

  constructor(view: EditorView) {
    this.view = view
    this.dom = document.createElement('div')
    this.dom.className = 'cm-slash-menu'
    this.dom.setAttribute('role', 'listbox')
    this.dom.style.position = 'absolute'
    this.dom.style.zIndex = '60'
    this.dom.style.display = 'none'
    this.dom.addEventListener('mousedown', (e) => {
      // Keep CM focus when clicking a menu item.
      e.preventDefault()
    })
    view.dom.appendChild(this.dom)
    this.sync()
  }

  update(update: ViewUpdate): void {
    const prev = update.startState.field(menuField)
    const next = update.state.field(menuField)
    if (prev === next && !update.docChanged && !update.geometryChanged) return
    this.sync()
  }

  destroy(): void {
    this.dom.remove()
  }

  sync(): void {
    const menu = this.view.state.field(menuField)
    if (!menu) {
      this.dom.style.display = 'none'
      this.lastMatches = []
      return
    }
    const matches = filterCommands(slashCommands.all(), menu.query)
    this.lastMatches = matches
    if (matches.length === 0) {
      this.dom.style.display = 'none'
      return
    }
    // Position above or below the cursor depending on viewport room.
    const coords = this.view.coordsAtPos(menu.from)
    if (!coords) {
      this.dom.style.display = 'none'
      return
    }
    const hostRect = this.view.dom.getBoundingClientRect()
    this.dom.style.display = 'block'
    this.dom.style.left = `${coords.left - hostRect.left}px`
    this.dom.style.top = `${coords.bottom - hostRect.top + 2}px`
    this.renderRows(matches, menu.highlight)
  }

  private renderRows(matches: SlashCommand[], highlight: number): void {
    // Simple rebuild — the palette is small so diffing isn't worth it.
    this.dom.textContent = ''
    let lastCategory: string | undefined
    matches.forEach((cmd, i) => {
      if (cmd.category && cmd.category !== lastCategory) {
        const header = document.createElement('div')
        header.className = 'cm-slash-menu__category'
        header.textContent = cmd.category
        this.dom.appendChild(header)
        lastCategory = cmd.category
      }
      const row = document.createElement('div')
      row.className = 'cm-slash-menu__row'
      if (i === highlight) row.classList.add('cm-slash-menu__row--active')
      row.dataset.slashCommandId = cmd.id
      row.setAttribute('role', 'option')
      row.setAttribute('aria-selected', i === highlight ? 'true' : 'false')
      row.addEventListener('mouseenter', () => {
        this.view.dispatch({ effects: updateMenu.of({ highlight: i }) })
      })
      row.addEventListener('click', () => {
        const menu = this.view.state.field(menuField)
        if (!menu) return
        applyCommand(this.view, menu, cmd)
        this.view.focus()
      })
      const glyph = document.createElement('span')
      glyph.className = 'cm-slash-menu__glyph'
      glyph.textContent = cmd.glyph ?? '•'
      const text = document.createElement('div')
      text.className = 'cm-slash-menu__text'
      const label = document.createElement('div')
      label.className = 'cm-slash-menu__label'
      label.textContent = cmd.label
      text.appendChild(label)
      if (cmd.description) {
        const desc = document.createElement('div')
        desc.className = 'cm-slash-menu__desc'
        desc.textContent = cmd.description
        text.appendChild(desc)
      }
      row.appendChild(glyph)
      row.appendChild(text)
      this.dom.appendChild(row)
    })
  }

  getMatches(): SlashCommand[] {
    return this.lastMatches
  }
}

// ── Public extension ─────────────────────────────────────────────────────────

export function slashCommandExt(): Extension {
  const pluginSpec = ViewPlugin.fromClass(SlashMenuPlugin, {
    decorations: (v: unknown) => {
      // Decoration set is derived from the menu field, not the plugin
      // itself; the plugin just drives the DOM overlay.
      void v
      return Decoration.none
    },
  })

  return [
    menuField,
    pluginSpec,
    // Derived decoration from the field so CSS can style the in-flight
    // slash query.
    EditorView.decorations.compute([menuField], (state) =>
      buildDecorations(state.field(menuField)),
    ),
    EditorView.domEventHandlers({
      keydown(event, view) {
        return handleKeydown(event, view)
      },
    }),
    keymap.of([
      {
        key: 'ArrowDown',
        run(view) {
          const menu = view.state.field(menuField)
          if (!menu) return false
          const matches = filterCommands(slashCommands.all(), menu.query)
          if (matches.length === 0) return false
          const next = (menu.highlight + 1) % matches.length
          view.dispatch({ effects: updateMenu.of({ highlight: next }) })
          return true
        },
      },
      {
        key: 'ArrowUp',
        run(view) {
          const menu = view.state.field(menuField)
          if (!menu) return false
          const matches = filterCommands(slashCommands.all(), menu.query)
          if (matches.length === 0) return false
          const next = (menu.highlight - 1 + matches.length) % matches.length
          view.dispatch({ effects: updateMenu.of({ highlight: next }) })
          return true
        },
      },
      {
        key: 'Enter',
        run(view) {
          const menu = view.state.field(menuField)
          if (!menu) return false
          const matches = filterCommands(slashCommands.all(), menu.query)
          const pick = matches[menu.highlight]
          if (!pick) return false
          applyCommand(view, menu, pick)
          return true
        },
      },
      {
        key: 'Tab',
        run(view) {
          const menu = view.state.field(menuField)
          if (!menu) return false
          const matches = filterCommands(slashCommands.all(), menu.query)
          const pick = matches[menu.highlight]
          if (!pick) return false
          applyCommand(view, menu, pick)
          return true
        },
      },
      {
        key: 'Escape',
        run(view) {
          const menu = view.state.field(menuField)
          if (!menu) return false
          view.dispatch({ effects: closeMenu.of() })
          return true
        },
      },
    ]),
  ]
}

function handleKeydown(event: KeyboardEvent, view: EditorView): boolean {
  if (event.key !== '/') return false
  if (event.ctrlKey || event.metaKey || event.altKey) return false
  const head = view.state.selection.main.head
  if (!canOpenAt(view.state, head)) return false
  // Defer the open effect until after the `/` character lands in the
  // doc — if we fire here the `from` would point at the position the
  // slash *will* occupy. Easier to piggyback on the natural insert:
  // don't preventDefault, but queue an effect that marks `from`.
  requestAnimationFrame(() => {
    const current = view.state.selection.main.head
    if (current < 1) return
    const slash = view.state.doc.sliceString(current - 1, current)
    if (slash !== '/') return
    view.dispatch({ effects: openMenu.of({ from: current - 1 }) })
  })
  return false
}

/** Style block injected into the app so the palette looks consistent
 *  across tabs without requiring the editor markdown stylesheet to
 *  grow slash-menu classes. Call from the plugin `activate`. */
export function installSlashMenuStyles(): () => void {
  const id = 'nexus-editor-slash-menu-styles'
  if (document.getElementById(id)) return () => undefined
  const style = document.createElement('style')
  style.id = id
  style.textContent = `
.cm-slash-menu {
  min-width: 260px;
  max-width: 360px;
  max-height: 320px;
  overflow-y: auto;
  background: var(--background-secondary, #2d2d2d);
  color: var(--text-normal, #e5e7eb);
  border: 1px solid var(--divider-color, #3f3f46);
  border-radius: 6px;
  box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
  font-family: var(--font-family, system-ui, sans-serif);
  font-size: 12px;
  padding: 4px 0;
}
.cm-slash-menu__category {
  padding: 6px 12px 2px;
  font-size: 10px;
  letter-spacing: 0.6px;
  text-transform: uppercase;
  color: var(--text-muted, #9ca3af);
}
.cm-slash-menu__row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  cursor: pointer;
}
.cm-slash-menu__row--active,
.cm-slash-menu__row:hover {
  background: var(--background-modifier-hover, #363636);
}
.cm-slash-menu__glyph {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: 4px;
  border: 1px solid var(--divider-color, #3f3f46);
  color: var(--text-muted, #9ca3af);
  font-size: 11px;
  flex-shrink: 0;
}
.cm-slash-menu__text {
  display: flex;
  flex-direction: column;
  min-width: 0;
}
.cm-slash-menu__label {
  color: var(--text-normal, #e5e7eb);
}
.cm-slash-menu__desc {
  color: var(--text-muted, #9ca3af);
  font-size: 11px;
}
.cm-slash-query {
  text-decoration: underline dotted var(--interactive-accent, #60a5fa);
  text-underline-offset: 2px;
}
`
  document.head.appendChild(style)
  return () => style.remove()
}

export type { Transaction }
