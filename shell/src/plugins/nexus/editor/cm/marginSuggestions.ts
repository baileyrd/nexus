// shell/src/plugins/nexus/editor/cm/marginSuggestions.ts
//
// BL-036 phase 2 — margin-glyph + diff-card CodeMirror extension for
// the AMB margin-suggestions engine (phase 1 lives in
// `shell/src/plugins/nexus/ai/marginSuggest{,Store}.ts`).
//
// Renders three things per active suggestion:
//
//   1. An inline `Decoration.mark` with a kind-specific class over
//      `[from, to)` so the underlying span has a soft underline /
//      highlight the user can hover.
//   2. A right-margin glyph (absolute-positioned div) at the line
//      anchoring the suggestion. Click expands.
//   3. A floating diff card next to the expanded glyph, showing
//      original vs replacement plus Accept / Dismiss buttons.
//      Reuses the BL-035 dismiss verb on the store.
//
// Phase 3 adds:
//   4. A wavy squiggle decoration for spelling / grammar (the kinds
//      that don't get a margin glyph because they're high-volume
//      and fine-grained). Rendered via the same `Decoration.mark`
//      pipeline — only the CSS for `cm-margin-suggest--{spelling,
//      grammar}` swaps from a dotted underline to a wavy SVG
//      background.
//   5. A native `contextmenu` listener — right-click inside ANY
//      suggestion span (including spelling / grammar that have no
//      glyph) surfaces a small Accept / Dismiss menu at the click
//      coords. Composes the same `buildAcceptTransaction` /
//      `dropOneEffect` machinery the diff card uses. The default
//      browser context menu is suppressed only when the click is
//      inside a suggestion — clicks outside still get the platform
//      menu.
//
// Drift safety: every store update + every doc-change transaction
// drops suggestions whose `[from, to)` no longer matches their
// captured `original`. Accepting a stale suggestion would clobber
// the user's edits — never do it.

import {
  StateEffect,
  StateField,
  type Extension,
  type Transaction,
} from '@codemirror/state'
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type PluginValue,
  type ViewUpdate,
} from '@codemirror/view'

import {
  useMarginSuggestStore,
  type Suggestion,
  type SuggestionKind,
} from '../../ai/marginSuggestStore'

/** Suggestion as held by the StateField — char offsets are LIVE
 *  (mapped through every doc-change transaction). The store's
 *  `Suggestion.rangeFrom/rangeTo` are snapshot-time offsets; this
 *  type's `from/to` track edits. */
export interface ResolvedSuggestion {
  id: string
  kind: SuggestionKind
  from: number
  to: number
  /** Original text in `[from, to)` at the snapshot the engine
   *  analyzed. The mapper drops the suggestion when the live slice
   *  no longer matches this. */
  original: string
  replacement: string | null
  message: string
}

interface MarginState {
  suggestions: ResolvedSuggestion[]
  /** Id of the suggestion whose diff card is currently expanded;
   *  null when no card is open. Cleared on doc-change so a stray
   *  edit collapses the card without an extra dispatch. */
  expandedId: string | null
}

const INITIAL_STATE: MarginState = { suggestions: [], expandedId: null }

// ── Effects ──────────────────────────────────────────────────────────────

/** Replace the resolved suggestion list. Fired by the store
 *  subscriber; tests fire it directly. */
const setResolvedEffect = StateEffect.define<ResolvedSuggestion[]>()

/** Toggle which suggestion's diff card is expanded. `null` collapses. */
const expandEffect = StateEffect.define<string | null>()

/** Drop a single suggestion from the field — fired on Accept /
 *  Dismiss in addition to the store-side mutation, so the gutter
 *  glyph disappears in the same tick the user clicked even if the
 *  store subscription is async. */
const dropOneEffect = StateEffect.define<string>()

// ── Resolver: store snapshot → ResolvedSuggestion[] ─────────────────────

/** Validate each store-shape suggestion against the current doc
 *  text and return the subset that anchors cleanly:
 *
 *   - rangeFrom/rangeTo within doc bounds
 *   - rangeFrom < rangeTo
 *   - doc.slice(rangeFrom, rangeTo) === original
 *
 *  Mismatched entries are dropped (the user has edited the span
 *  since the pass — the engine's `original` is stale). Exposed for
 *  tests + the store-subscription wiring. */
export function resolveSuggestions(
  storeSuggestions: ReadonlyArray<Suggestion>,
  docText: string,
): ResolvedSuggestion[] {
  const out: ResolvedSuggestion[] = []
  const docLen = docText.length
  for (const s of storeSuggestions) {
    if (s.rangeFrom < 0 || s.rangeTo > docLen) continue
    if (s.rangeFrom >= s.rangeTo) continue
    if (docText.slice(s.rangeFrom, s.rangeTo) !== s.original) continue
    out.push({
      id: s.id,
      kind: s.kind,
      from: s.rangeFrom,
      to: s.rangeTo,
      original: s.original,
      replacement: s.replacement,
      message: s.message,
    })
  }
  return out
}

// ── State field ─────────────────────────────────────────────────────────

/** Map the resolved-suggestion list through a transaction's doc
 *  changes. Each suggestion's `[from, to)` is remapped (`mapPos`
 *  with `assoc=1` for `from`, `-1` for `to` so the range collapses
 *  toward the change rather than expanding through it). After
 *  remapping, verify the live slice still equals `original`; drop
 *  on mismatch. Exposed for tests. */
export function mapSuggestionsThroughTransaction(
  suggestions: ResolvedSuggestion[],
  tr: Transaction,
): ResolvedSuggestion[] {
  if (!tr.docChanged) return suggestions
  const out: ResolvedSuggestion[] = []
  const doc = tr.state.doc
  for (const s of suggestions) {
    const newFrom = tr.changes.mapPos(s.from, 1)
    const newTo = tr.changes.mapPos(s.to, -1)
    if (newFrom >= newTo) continue
    if (newTo > doc.length) continue
    if (doc.sliceString(newFrom, newTo) !== s.original) continue
    out.push({ ...s, from: newFrom, to: newTo })
  }
  return out
}

const marginField = StateField.define<MarginState>({
  create: () => INITIAL_STATE,
  update(value, tr) {
    let suggestions = value.suggestions
    let expandedId = value.expandedId

    // Doc edits remap + revalidate ranges; an edit also collapses
    // any open card (the user is typing — they're not interacting
    // with it).
    if (tr.docChanged) {
      suggestions = mapSuggestionsThroughTransaction(suggestions, tr)
      // If the expanded suggestion was dropped by remap, clear.
      if (expandedId && !suggestions.some((s) => s.id === expandedId)) {
        expandedId = null
      } else if (expandedId !== null) {
        // Edit while a card is open — collapse.
        expandedId = null
      }
    }

    for (const e of tr.effects) {
      if (e.is(setResolvedEffect)) {
        suggestions = e.value
        // A store-driven replacement implies a fresh pass — close
        // any open card so it doesn't dangle on a now-removed id.
        expandedId = null
      } else if (e.is(expandEffect)) {
        // Only expand ids the field actually knows about; cheap
        // guard against a click-after-dismiss race.
        if (e.value === null) {
          expandedId = null
        } else if (suggestions.some((s) => s.id === e.value)) {
          expandedId = e.value
        }
      } else if (e.is(dropOneEffect)) {
        const id = e.value
        suggestions = suggestions.filter((s) => s.id !== id)
        if (expandedId === id) expandedId = null
      }
    }

    return { suggestions, expandedId }
  },

  provide: (f) =>
    EditorView.decorations.compute([f], (state) => buildDecorations(state.field(f).suggestions)),
})

/** Compose the inline `Decoration.mark` set from the field's
 *  resolved suggestions. The mark class layers per-kind styling on
 *  top of the shared `cm-margin-suggest` base so phase 3's squiggle
 *  for spelling / grammar is a pure CSS swap. Exposed for tests. */
export function buildDecorations(
  suggestions: ReadonlyArray<ResolvedSuggestion>,
): DecorationSet {
  const ranges = suggestions
    .slice()
    .sort((a, b) => a.from - b.from || a.to - b.to)
    .map((s) =>
      Decoration.mark({
        class: `cm-margin-suggest cm-margin-suggest--${s.kind}`,
        attributes: { 'data-margin-suggest-id': s.id },
      }).range(s.from, s.to),
    )
  return Decoration.set(ranges)
}

// ── Accept-side helper ──────────────────────────────────────────────────

/** Build the transaction spec that applies a suggestion's
 *  `replacement` over `[from, to)`. Returns `null` for fact-check
 *  (replacement === null) — those suggestions are annotation-only;
 *  Accept simply dismisses on the store side.
 *
 *  Splitting the spec from `dispatch` lets unit tests assert on the
 *  shape without standing up a real EditorView. */
export function buildAcceptTransaction(
  suggestion: ResolvedSuggestion,
): { changes: { from: number; to: number; insert: string }; effects: StateEffect<string> } | null {
  if (suggestion.replacement === null) return null
  return {
    changes: {
      from: suggestion.from,
      to: suggestion.to,
      insert: suggestion.replacement,
    },
    // Drop the suggestion in the same transaction so the glyph
    // disappears atomically with the doc edit.
    effects: dropOneEffect.of(suggestion.id),
  }
}

// ── Suggestion-at-position lookup ────────────────────────────────────────

/** Find the suggestion in `state` whose anchored range covers `pos`,
 *  if any. Used by the right-click contextmenu (phase 3) to surface
 *  Accept / Dismiss for the suggestion under the cursor — including
 *  spelling / grammar kinds that don't get a margin glyph.
 *
 *  Inclusive on `from`, exclusive on `to` — matches the semantics
 *  of the `Decoration.mark` range. When multiple suggestions cover
 *  the same offset (rare; the engine dedupes by `kind|original`
 *  but two different kinds could overlap), the first in registration
 *  order wins; that's deterministic enough for the menu's "single
 *  active suggestion" UX. */
export function suggestionAtPos(
  suggestions: ReadonlyArray<ResolvedSuggestion>,
  pos: number,
): ResolvedSuggestion | null {
  for (const s of suggestions) {
    if (pos >= s.from && pos < s.to) return s
  }
  return null
}

// ── Contextmenu spec ─────────────────────────────────────────────────────

/** A single row to render in the right-click menu over a suggestion.
 *  Pure-data shape so the DOM render in `MarginSuggestionsView` is
 *  separable from the spec — tests assert on the spec without
 *  standing up DOM. */
export interface ContextMenuRow {
  /** Stable id for keyed rendering / test assertions. */
  id: 'accept' | 'dismiss' | 'show-diff'
  /** Visible label. Includes the kind name when relevant so the
   *  user knows what they're acting on at a glance. */
  label: string
}

/** Build the row list for a contextmenu over `suggestion`. Rules:
 *
 *   - "Accept fix" appears iff the suggestion has a non-null
 *     replacement (rephrase / tighten / spelling / grammar). Hidden
 *     for fact-check, which is annotation-only.
 *   - "Show diff" appears iff there's a replacement to compare
 *     against AND the suggestion isn't already in the diff-card-
 *     visible set (rephrase / tighten / fact-check, where the
 *     margin glyph + click already does this). For spelling /
 *     grammar (no glyph), the right-click is the ONLY way to see
 *     the proposed fix before applying.
 *   - "Dismiss" always appears — the user can always close out.
 *
 *  Exposed for tests; the contextmenu handler in
 *  `MarginSuggestionsView` consumes this. */
export function buildContextMenuRows(suggestion: ResolvedSuggestion): ContextMenuRow[] {
  const rows: ContextMenuRow[] = []
  const hasReplacement = suggestion.replacement !== null

  if (hasReplacement) {
    rows.push({ id: 'accept', label: `Accept ${suggestion.kind} fix` })
  }
  // "Show diff" — only for kinds that don't surface a margin glyph
  // (spelling / grammar). The other kinds already have a glyph
  // click that opens the diff card; surfacing it twice would be
  // noise.
  if (hasReplacement && (suggestion.kind === 'spelling' || suggestion.kind === 'grammar')) {
    rows.push({ id: 'show-diff', label: 'Show diff' })
  }
  rows.push({ id: 'dismiss', label: 'Dismiss suggestion' })
  return rows
}

// ── ViewPlugin: glyph layer + diff card DOM ─────────────────────────────

const KIND_GLYPH: Record<SuggestionKind, string> = {
  rephrase: '↻',
  tighten: '✂',
  'fact-check': '?',
  // Spelling/grammar render through the underline only in phase 2 —
  // glyphs would clutter for high-volume kinds. The map is exhaustive
  // so phase 3 only has to update the icon, not the structure.
  spelling: 'A',
  grammar: 'G',
}

/** Kinds that surface a margin glyph in phase 2. Spelling/grammar
 *  are reserved for phase 3's squiggle treatment. */
const GLYPH_KINDS: ReadonlySet<SuggestionKind> = new Set<SuggestionKind>([
  'rephrase',
  'tighten',
  'fact-check',
])

class MarginSuggestionsView implements PluginValue {
  private readonly view: EditorView
  private readonly relpath: string
  private readonly glyphLayer: HTMLDivElement
  private readonly cardLayer: HTMLDivElement
  /** Phase 3 — separate layer for the right-click contextmenu so a
   *  visible card and a visible menu can co-exist without DOM
   *  surgery (rare in practice — opening the menu collapses the
   *  card — but the layer separation simplifies hit-testing in
   *  `onGlobalMouseDown`). */
  private readonly menuLayer: HTMLDivElement
  private readonly storeUnsub: () => void
  /** Track the active doc text so the resolver only fires when the
   *  store actually changes; resists a thundering herd if the store
   *  emits transient "pending" status updates. */
  private lastSyncedSuggestionsRef: ReadonlyArray<Suggestion> | null = null

  constructor(view: EditorView, relpath: string) {
    this.view = view
    this.relpath = relpath

    if (getComputedStyle(view.dom).position === 'static') {
      view.dom.style.position = 'relative'
    }

    this.glyphLayer = document.createElement('div')
    this.glyphLayer.className = 'cm-margin-suggest-glyphs'
    this.glyphLayer.style.position = 'absolute'
    this.glyphLayer.style.right = '0'
    this.glyphLayer.style.top = '0'
    this.glyphLayer.style.bottom = '0'
    this.glyphLayer.style.width = '24px'
    this.glyphLayer.style.pointerEvents = 'none'
    this.glyphLayer.style.zIndex = '50'
    view.dom.appendChild(this.glyphLayer)

    this.cardLayer = document.createElement('div')
    this.cardLayer.className = 'cm-margin-suggest-card-layer'
    this.cardLayer.style.position = 'absolute'
    this.cardLayer.style.zIndex = '70'
    this.cardLayer.style.display = 'none'
    // Card layer ignores its own clicks for parent capture but
    // forwards clicks inside (Accept / Dismiss buttons).
    this.cardLayer.addEventListener('mousedown', (e) => e.stopPropagation())
    view.dom.appendChild(this.cardLayer)

    this.menuLayer = document.createElement('div')
    this.menuLayer.className = 'cm-margin-suggest-menu-layer'
    this.menuLayer.style.position = 'absolute'
    this.menuLayer.style.zIndex = '80'
    this.menuLayer.style.display = 'none'
    this.menuLayer.addEventListener('mousedown', (e) => e.stopPropagation())
    view.dom.appendChild(this.menuLayer)

    view.dom.addEventListener('contextmenu', this.onContextMenu)
    document.addEventListener('mousedown', this.onGlobalMouseDown)

    // Subscribe to the zustand store. The subscriber fires on every
    // store mutation; we filter for the doc path and dispatch a
    // setResolvedEffect when the suggestion list changes.
    this.storeUnsub = useMarginSuggestStore.subscribe((state, prev) => {
      this.maybeSync(state, prev)
    })
    // Initial pull deferred so view.dispatch is not called inside CM's
    // construction update — dispatching while CM is updating throws.
    queueMicrotask(() => this.maybeSync(useMarginSuggestStore.getState(), null))

    this.render()
  }

  private maybeSync(
    state: ReturnType<typeof useMarginSuggestStore.getState>,
    prev: ReturnType<typeof useMarginSuggestStore.getState> | null,
  ): void {
    // Only resolve when the doc this editor instance is bound to is
    // the one the store currently holds. Multiple tabs share the
    // singleton store; a stale tab paints nothing.
    if (state.currentDocPath !== this.relpath) {
      // If we previously had suggestions and the store has moved
      // on, clear the field so glyphs from a prior pass don't
      // outlive their relevance.
      if (this.lastSyncedSuggestionsRef && this.lastSyncedSuggestionsRef.length > 0) {
        this.lastSyncedSuggestionsRef = null
        this.view.dispatch({ effects: setResolvedEffect.of([]) })
      }
      return
    }
    if (prev && state.suggestions === prev.suggestions) return
    this.lastSyncedSuggestionsRef = state.suggestions
    const docText = this.view.state.doc.toString()
    const resolved = resolveSuggestions(state.suggestions, docText)
    this.view.dispatch({ effects: setResolvedEffect.of(resolved) })
  }

  update(u: ViewUpdate): void {
    // Re-render glyph positions / card position when the doc, viewport,
    // or our field state changes. Cheap — DOM diff is element count
    // bounded by the per-pass cap (6).
    if (
      u.docChanged ||
      u.viewportChanged ||
      u.geometryChanged ||
      u.startState.field(marginField) !== u.state.field(marginField)
    ) {
      const { suggestions, expandedId } = u.state.field(marginField)
      const expandedTarget = expandedId ? suggestions.find(s => s.id === expandedId) : null
      this.view.requestMeasure({
        read: (view) => {
          const editorRect = view.dom.getBoundingClientRect()
          const glyphLayout = suggestions
            .filter(s => GLYPH_KINDS.has(s.kind))
            .map(s => ({ s, coords: view.coordsAtPos(s.from) }))
          const cardCoords = expandedTarget ? view.coordsAtPos(expandedTarget.from) : null
          return { editorRect, glyphLayout, cardCoords }
        },
        write: ({ editorRect, glyphLayout, cardCoords }) => {
          this.renderGlyphs(glyphLayout, editorRect)
          this.renderCard(suggestions, expandedId, cardCoords, editorRect)
        },
      })
    }
  }

  destroy(): void {
    this.storeUnsub()
    this.view.dom.removeEventListener('contextmenu', this.onContextMenu)
    document.removeEventListener('mousedown', this.onGlobalMouseDown)
    this.glyphLayer.remove()
    this.cardLayer.remove()
    this.menuLayer.remove()
  }

  // ── DOM render ────────────────────────────────────────────────────────

  private render(): void {
    const { suggestions, expandedId } = this.view.state.field(marginField)
    const editorRect = this.view.dom.getBoundingClientRect()
    const glyphLayout = suggestions
      .filter(s => GLYPH_KINDS.has(s.kind))
      .map(s => ({ s, coords: this.view.coordsAtPos(s.from) }))
    const expandedTarget = expandedId ? suggestions.find(s => s.id === expandedId) : null
    const cardCoords = expandedTarget ? this.view.coordsAtPos(expandedTarget.from) : null
    this.renderGlyphs(glyphLayout, editorRect)
    this.renderCard(suggestions, expandedId, cardCoords, editorRect)
  }

  private renderGlyphs(
    layout: Array<{ s: ResolvedSuggestion; coords: { top: number; bottom: number; left: number; right: number } | null }>,
    editorRect: { top: number },
  ): void {
    // Naïve full-rebuild — list is small (cap 6) so DOM diffing
    // would be over-engineered. Each glyph is pointer-events: auto
    // so click works through the layer's pointer-events: none.
    this.glyphLayer.replaceChildren()
    for (const { s, coords } of layout) {
      if (!coords) continue
      const top = coords.top - editorRect.top
      const btn = document.createElement('button')
      btn.type = 'button'
      btn.className = `cm-margin-suggest-glyph cm-margin-suggest-glyph--${s.kind}`
      btn.dataset.marginSuggestId = s.id
      btn.style.position = 'absolute'
      btn.style.top = `${top}px`
      btn.style.right = '4px'
      btn.style.pointerEvents = 'auto'
      btn.title = s.message
      btn.textContent = KIND_GLYPH[s.kind]
      btn.addEventListener('click', (e) => {
        e.preventDefault()
        e.stopPropagation()
        const cur = this.view.state.field(marginField).expandedId
        // Toggle behaviour — clicking the same glyph collapses.
        this.view.dispatch({
          effects: expandEffect.of(cur === s.id ? null : s.id),
        })
      })
      this.glyphLayer.appendChild(btn)
    }
  }

  private renderCard(
    suggestions: ReadonlyArray<ResolvedSuggestion>,
    expandedId: string | null,
    coords: { top: number; bottom: number; left: number; right: number } | null,
    editorRect: { top: number },
  ): void {
    if (expandedId === null) {
      this.cardLayer.style.display = 'none'
      this.cardLayer.replaceChildren()
      return
    }
    const target = suggestions.find((s) => s.id === expandedId)
    if (!target) {
      this.cardLayer.style.display = 'none'
      this.cardLayer.replaceChildren()
      return
    }

    if (!coords) {
      this.cardLayer.style.display = 'none'
      return
    }

    const card = document.createElement('div')
    card.className = `cm-margin-suggest-card cm-margin-suggest-card--${target.kind}`

    const header = document.createElement('div')
    header.className = 'cm-margin-suggest-card-header'
    header.textContent = target.kind
    card.appendChild(header)

    const message = document.createElement('div')
    message.className = 'cm-margin-suggest-card-message'
    message.textContent = target.message
    card.appendChild(message)

    if (target.replacement !== null) {
      const diff = document.createElement('div')
      diff.className = 'cm-margin-suggest-card-diff'

      const oldLine = document.createElement('div')
      oldLine.className = 'cm-margin-suggest-card-diff-old'
      oldLine.textContent = target.original
      diff.appendChild(oldLine)

      const newLine = document.createElement('div')
      newLine.className = 'cm-margin-suggest-card-diff-new'
      newLine.textContent = target.replacement
      diff.appendChild(newLine)

      card.appendChild(diff)
    }

    const actions = document.createElement('div')
    actions.className = 'cm-margin-suggest-card-actions'

    if (target.replacement !== null) {
      const accept = document.createElement('button')
      accept.type = 'button'
      accept.className = 'cm-margin-suggest-card-accept'
      accept.textContent = 'Accept'
      accept.addEventListener('click', (e) => {
        e.preventDefault()
        e.stopPropagation()
        this.applyAccept(target)
      })
      actions.appendChild(accept)
    }

    const dismiss = document.createElement('button')
    dismiss.type = 'button'
    dismiss.className = 'cm-margin-suggest-card-dismiss'
    dismiss.textContent = 'Dismiss'
    dismiss.addEventListener('click', (e) => {
      e.preventDefault()
      e.stopPropagation()
      this.applyDismiss(target.id)
    })
    actions.appendChild(dismiss)

    card.appendChild(actions)

    // Position card right of the glyph, vertically aligned to the
    // suggestion's first line. Layer is fixed within the editor's
    // relative-positioned dom.
    this.cardLayer.replaceChildren(card)
    this.cardLayer.style.display = 'block'
    this.cardLayer.style.top = `${coords.top - editorRect.top}px`
    this.cardLayer.style.right = '32px'
  }

  // ── Accept / Dismiss ──────────────────────────────────────────────────

  private applyAccept(target: ResolvedSuggestion): void {
    const spec = buildAcceptTransaction(target)
    if (spec) {
      // The transaction goes through the editor's transaction-bridge
      // pipeline like any user edit; if a session is active the
      // bridge mirrors it to the kernel for undo/redo coverage.
      this.view.dispatch(spec)
    } else {
      // Fact-check has no replacement — Accept is a UX no-op beyond
      // dismissing. Keep the call site uniform.
      this.view.dispatch({ effects: dropOneEffect.of(target.id) })
    }
    useMarginSuggestStore.getState().accept(target.id)
  }

  private applyDismiss(id: string): void {
    this.view.dispatch({ effects: dropOneEffect.of(id) })
    useMarginSuggestStore.getState().dismiss(id)
  }

  // ── Outside-click closes the card + the contextmenu ─────────────────

  private onGlobalMouseDown = (e: MouseEvent): void => {
    const target = e.target as Node | null
    if (!target) return
    const inCard = this.cardLayer.contains(target)
    const inGlyph = this.glyphLayer.contains(target)
    const inMenu = this.menuLayer.contains(target)
    if (!inMenu && this.menuLayer.style.display !== 'none') {
      this.closeMenu()
    }
    if (inCard || inGlyph || inMenu) return
    if (this.view.state.field(marginField).expandedId !== null) {
      this.view.dispatch({ effects: expandEffect.of(null) })
    }
  }

  // ── Right-click contextmenu (phase 3) ────────────────────────────────

  private onContextMenu = (e: MouseEvent): void => {
    // Map screen coords → CM doc offset; if the click is outside
    // the active suggestion list we leave the platform menu alone
    // (platform paste / spell-check shouldn't be eaten).
    const pos = this.view.posAtCoords({ x: e.clientX, y: e.clientY })
    if (pos === null) return
    const { suggestions } = this.view.state.field(marginField)
    const target = suggestionAtPos(suggestions, pos)
    if (!target) return
    e.preventDefault()
    e.stopPropagation()
    // Close any open diff card so the menu has the user's full
    // attention; the menu's "Show diff" row reopens it explicitly.
    if (this.view.state.field(marginField).expandedId !== null) {
      this.view.dispatch({ effects: expandEffect.of(null) })
    }
    this.openMenu(target, e.clientX, e.clientY)
  }

  private openMenu(target: ResolvedSuggestion, clientX: number, clientY: number): void {
    const editorRect = this.view.dom.getBoundingClientRect()
    const rows = buildContextMenuRows(target)

    const menu = document.createElement('div')
    menu.className = `cm-margin-suggest-menu cm-margin-suggest-menu--${target.kind}`

    for (const row of rows) {
      const item = document.createElement('button')
      item.type = 'button'
      item.className = `cm-margin-suggest-menu-row cm-margin-suggest-menu-row--${row.id}`
      item.textContent = row.label
      item.addEventListener('click', (ev) => {
        ev.preventDefault()
        ev.stopPropagation()
        this.handleMenuRow(row.id, target)
      })
      menu.appendChild(item)
    }

    this.menuLayer.replaceChildren(menu)
    this.menuLayer.style.display = 'block'
    // Position relative to the editor's dom rect — clientX/Y are
    // viewport coords, the layer is positioned within view.dom which
    // is `position: relative` (set in the constructor).
    this.menuLayer.style.left = `${clientX - editorRect.left}px`
    this.menuLayer.style.top = `${clientY - editorRect.top}px`
  }

  private closeMenu(): void {
    this.menuLayer.style.display = 'none'
    this.menuLayer.replaceChildren()
  }

  private handleMenuRow(
    rowId: ContextMenuRow['id'],
    target: ResolvedSuggestion,
  ): void {
    if (rowId === 'accept') {
      this.applyAccept(target)
    } else if (rowId === 'dismiss') {
      this.applyDismiss(target.id)
    } else if (rowId === 'show-diff') {
      // Reopen the diff card the user dismissed when the menu opened.
      // The card already knows how to render every kind including
      // spelling / grammar — the only reason those don't get a glyph
      // is gutter clutter.
      this.view.dispatch({ effects: expandEffect.of(target.id) })
    }
    this.closeMenu()
  }
}

// ── Extension factory ───────────────────────────────────────────────────

export interface MarginSuggestionsOptions {
  /** Forge-relative path of the doc this editor instance is showing.
   *  Used to filter the singleton store — only suggestions whose
   *  `currentDocPath` matches will paint. */
  relpath: string
}

export function marginSuggestionsExt(opts: MarginSuggestionsOptions): Extension {
  return [
    marginField,
    ViewPlugin.define((view) => new MarginSuggestionsView(view, opts.relpath)),
  ]
}

/** Install the styles for the glyph layer + diff card. Mirrors the
 *  `installBlockHandleStyles` pattern (`shell/src/plugins/nexus/
 *  editor/cm/blockHandle.ts`) — colour vars fall back to the dark-
 *  theme defaults when the active theme doesn't override them.
 *  Returns a disposer the editor plugin's `activate()` doesn't
 *  bother to call (styles outlive a plugin reload). */
export function installMarginSuggestStyles(): () => void {
  const id = 'nexus-editor-margin-suggest-styles'
  if (document.getElementById(id)) return () => undefined
  const style = document.createElement('style')
  style.id = id
  style.textContent = `
.cm-margin-suggest {
  border-bottom: 1px dashed var(--ai-accent, #60a5fa);
  background: rgba(96, 165, 250, 0.06);
}
.cm-margin-suggest--rephrase {
  border-bottom-color: var(--ai-accent-rephrase, #60a5fa);
}
.cm-margin-suggest--tighten {
  border-bottom-color: var(--ai-accent-tighten, #a78bfa);
}
.cm-margin-suggest--fact-check {
  border-bottom-color: var(--ai-accent-warning, #fbbf24);
  background: rgba(251, 191, 36, 0.08);
}
.cm-margin-suggest--spelling,
.cm-margin-suggest--grammar {
  /* Phase 3 — wavy underline. Spec-compliant approach in a single
   * declaration: a 6×3 SVG repeated horizontally as a background
   * image so we don't fight CodeMirror's text rendering. The SVG
   * is encoded inline (no external request, no theme override
   * needed) and can be tinted by swapping the stroke colour. */
  border-bottom: none;
  background-color: transparent;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 6 3' width='6' height='3'><path d='M 0 1.5 Q 1.5 0 3 1.5 T 6 1.5' fill='none' stroke='%23fbbf24' stroke-width='0.8'/></svg>");
  background-repeat: repeat-x;
  background-position: bottom left;
  background-size: 6px 3px;
  padding-bottom: 1px;
}
.cm-margin-suggest--spelling {
  /* Spelling kept on the warning palette; the kind difference is
   * already conveyed by the contextmenu label so we don't need a
   * second colour to distinguish it from grammar. */
}
.cm-margin-suggest--grammar {
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 6 3' width='6' height='3'><path d='M 0 1.5 Q 1.5 0 3 1.5 T 6 1.5' fill='none' stroke='%23a78bfa' stroke-width='0.8'/></svg>");
}
.cm-margin-suggest-glyph {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 1px solid var(--divider-color);
  background: var(--background-secondary);
  color: var(--text-normal);
  font-size: 11px;
  line-height: 1;
  cursor: pointer;
  opacity: 0.65;
  transition: opacity 120ms ease, background 120ms ease;
  padding: 0;
}
.cm-margin-suggest-glyph:hover {
  opacity: 1;
  background: var(--background-modifier-hover);
}
.cm-margin-suggest-glyph--rephrase {
  color: var(--ai-accent-rephrase, #60a5fa);
}
.cm-margin-suggest-glyph--tighten {
  color: var(--ai-accent-tighten, #a78bfa);
}
.cm-margin-suggest-glyph--fact-check {
  color: var(--ai-accent-warning, #fbbf24);
}
.cm-margin-suggest-card {
  min-width: 240px;
  max-width: 360px;
  background: var(--background-secondary);
  color: var(--text-normal);
  border: 1px solid var(--divider-color);
  border-radius: 6px;
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.35);
  font-family: var(--font-family, system-ui, sans-serif);
  font-size: 12px;
  padding: 8px 10px;
}
.cm-margin-suggest-card-header {
  font-size: 10px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  margin-bottom: 4px;
}
.cm-margin-suggest-card-message {
  margin-bottom: 6px;
  line-height: 1.4;
}
.cm-margin-suggest-card-diff {
  border: 1px solid var(--divider-color);
  border-radius: 4px;
  margin-bottom: 8px;
  font-family: var(--font-family-mono, ui-monospace, monospace);
  font-size: 11px;
  overflow: hidden;
}
.cm-margin-suggest-card-diff-old {
  padding: 4px 6px;
  background: rgba(244, 63, 94, 0.10);
  text-decoration: line-through;
  text-decoration-color: rgba(244, 63, 94, 0.6);
}
.cm-margin-suggest-card-diff-new {
  padding: 4px 6px;
  background: rgba(34, 197, 94, 0.10);
  border-top: 1px solid var(--divider-color);
}
.cm-margin-suggest-card-actions {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
}
.cm-margin-suggest-card-actions button {
  padding: 4px 10px;
  border-radius: 4px;
  border: 1px solid var(--divider-color);
  background: var(--background-primary);
  color: var(--text-normal);
  font-size: 11px;
  cursor: pointer;
}
.cm-margin-suggest-card-accept {
  background: var(--ai-accent, #60a5fa);
  color: var(--background-primary);
  border-color: var(--ai-accent, #60a5fa);
}
.cm-margin-suggest-card-actions button:hover {
  filter: brightness(1.1);
}
.cm-margin-suggest-menu {
  min-width: 180px;
  background: var(--background-secondary);
  color: var(--text-normal);
  border: 1px solid var(--divider-color);
  border-radius: 6px;
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.35);
  font-family: var(--font-family, system-ui, sans-serif);
  font-size: 12px;
  padding: 4px 0;
}
.cm-margin-suggest-menu-row {
  display: block;
  width: 100%;
  text-align: left;
  padding: 6px 12px;
  background: transparent;
  border: 0;
  color: inherit;
  font: inherit;
  cursor: pointer;
}
.cm-margin-suggest-menu-row:hover {
  background: var(--background-modifier-hover);
}
.cm-margin-suggest-menu-row--dismiss {
  border-top: 1px solid var(--divider-color);
  margin-top: 2px;
  padding-top: 6px;
}
`
  document.head.appendChild(style)
  return () => {
    style.remove()
  }
}

/** Test-only access to the field + effects so unit tests can drive
 *  state without standing up a full EditorView's DOM. */
export const __test__ = {
  marginField,
  setResolvedEffect,
  expandEffect,
  dropOneEffect,
}
