import { useEffect, useMemo, useRef, useState } from 'react'
import { EditorView as CMEditorView } from '@codemirror/view'
import { useEditorStore, type EditorTab, type EditorTabMode } from './editorStore'
import { renderMarkdown, hydrateFencedCode } from './markdownRender'
import { eventBus } from '../../../host/EventBus'
import { useOutlineStore } from '../outline/outlineStore'
import { Icon } from '../../../icons'
import { useWorkspaceField, workspace, type Tabs } from '../../../workspace'
import { getEditorRuntime, setActiveCmView } from './runtime'
import { CodeMirrorHost, type CodeMirrorHostHandle } from './cm/CodeMirrorHost'
import { transactionBridge } from './cm/transactionBridge'
import { getEditorMode, pickLanguageExtension } from './codeMode'
import { slashCommandExt } from './cm/slashCommand'
import { blockSelectionExt } from './cm/blockSelection'
import { multiCursorPromoteExt } from './cm/multiCursorPromote'
import { blockHandleExt } from './cm/blockHandle'
import { inputRulesExt } from './cm/inputRules'
import { inlineToolbarExt } from './cm/inlineToolbar'
import { livePreviewExt } from './cm/livePreview'
import { databaseViewExt } from './cm/databaseViewDecorations'
import { blockLinkNavExt } from './cm/blockLinkNav'
import { ghostCompletionExt } from './cm/ghostCompletion'
import { linkSuggestExt } from './cm/linkSuggest'
import { marginSuggestionsExt } from './cm/marginSuggestions'
import { marginSuggestTriggerExt } from './cm/marginSuggestTrigger'
import { getRegistry } from '../../../host/shellRegistry'
import { ContextMenu } from '../../../shell/ContextMenu'
import { buildTabContextMenu } from './TabContextMenu'
import './markdown.css'
import './livePreview.css'

/** Untitled placeholder relpaths have no kernel session; treat them
 *  locally only. Mirrors the predicate in `sessionManager.ts`. */
function isUntitled(relpath: string): boolean {
  return /^untitled-\d+$/i.test(relpath)
}

/**
 * Scroll a CodeMirror view so that `line1` (1-based) lands at the top
 * of the viewport. Clamps to doc bounds so callers can pass stale
 * heading line numbers without blowing up. Kept local to the editor
 * view — the helper is load-bearing for outline → source scrolling
 * but isn't generally useful outside this file.
 */
function viewToLine(view: CMEditorView, line1: number): void {
  const total = view.state.doc.lines
  if (total === 0) return
  const clamped = Math.max(1, Math.min(line1, total))
  const pos = view.state.doc.line(clamped).from
  view.dispatch({
    effects: CMEditorView.scrollIntoView(pos, { y: 'start' }),
  })
}

/**
 * Untitled tabs use `untitled-N` as their relpath placeholder. Such
 * tabs don't have real path segments to walk — the breadcrumb should
 * just show the tab name with no chevron trail.
 */
function isUntitledRelpath(relpath: string): boolean {
  return /^untitled-\d+$/i.test(relpath)
}

/**
 * Split a forge-relative path into segments. Forward-slash separated
 * per the `files:open` contract; we defensively split on backslashes
 * too so pasted Windows paths don't render as a single blob.
 */
function splitPathSegments(relpath: string): string[] {
  return relpath.split(/[\\/]+/).filter((s) => s.length > 0)
}

/**
 * Reverse contract of `editor:scrollToHeading`: the editor reports the
 * topmost heading currently at/above the visible region so the outline
 * can highlight that row. `index` is null when there are no headings or
 * the editor has scrolled above the first heading.
 */
const EVENT_ACTIVE_HEADING_CHANGED = 'editor:activeHeadingChanged'

/** Pixel tolerance below the scroll-container top: a heading whose top
 *  is within `[wrapTop, wrapTop + ACTIVE_HEADING_OFFSET]` still counts
 *  as "above the fold". Avoids flicker right at the boundary. */
const ACTIVE_HEADING_OFFSET = 8

/**
 * Outline → editor scroll contract. The outline plugin emits
 * `editor:scrollToHeading` with the 0-based heading index (among all
 * headings in the active doc) and the 1-based source line number.
 * Preview mode scrolls the Nth heading element into view; source mode
 * scrolls the CodeMirror view to the matching line.
 */
interface ScrollToHeadingPayload {
  headingId?: string
  line: number
  index: number
}

interface EditorViewProps {
  /** Relpath this leaf is bound to — sourced from the hosting
   *  `MarkdownView`'s `state.relpath`. Undefined for leaves that
   *  haven't been assigned a file yet (empty state). */
  relpath: string | undefined
  /** The workspace leaf ID — used to locate this leaf in its Tabs strip
   *  so the ← → nav buttons can move it left or right. */
  leafId: string
  onRetry: (relpath: string) => void
}

/** Returns the position of `leafId` within its Tabs strip, or null if not found.
 *  Re-evaluates on every `layout-change` so the button enabled state tracks moves. */
function useTabPosition(leafId: string): { tabsId: string; index: number; total: number } | null {
  const [pos, setPos] = useState<{ tabsId: string; index: number; total: number } | null>(null)
  useEffect(() => {
    function compute() {
      const leaf = workspace.leaves.get(leafId)
      if (!leaf || leaf.parent.kind !== 'tabs') { setPos(null); return }
      const tabs = leaf.parent as Tabs
      const index = tabs.leaves.findIndex((l) => l.id === leafId)
      setPos(index >= 0 ? { tabsId: tabs.id, index, total: tabs.leaves.length } : null)
    }
    compute()
    return workspace.on('layout-change', compute)
  }, [leafId])
  return pos
}

function isMarkdown(name: string): boolean {
  return /\.(md|markdown|mdx)$/i.test(name)
}

function isHtml(name: string): boolean {
  return /\.(html?|xhtml)$/i.test(name)
}

// Override styles appended to HTML files so the iframe always exposes
// scrollbars when content overflows. webkit2gtk (Tauri on Linux) hides
// overlay scrollbars by default, which made wide HTML docs appear
// truncated with no way to reach the right edge.
const HTML_VIEWER_OVERRIDES = `
<style>
  html, body { overflow: auto !important; }
  ::-webkit-scrollbar { width: 12px; height: 12px; }
  ::-webkit-scrollbar-thumb { background: rgba(127,127,127,0.5); border-radius: 6px; }
  ::-webkit-scrollbar-thumb:hover { background: rgba(127,127,127,0.75); }
  ::-webkit-scrollbar-track { background: transparent; }
</style>`

function withHtmlViewerOverrides(content: string): string {
  // Append rather than prepend so our rules win the cascade on ties and
  // we don't disturb a leading `<!doctype>` declaration.
  return content + HTML_VIEWER_OVERRIDES
}

/**
 * Editor view: tab row with per-tab dirty dot + a mode-toggle button
 * at the right end of the tab row, above a body that renders the
 * active tab either as markdown/<pre> (preview) or as a CodeMirror
 * view (source).
 *
 * Empty, loading, and error states are computed per-tab so a failed
 * load on one tab doesn't bleed into any neighbour.
 */
export function EditorView({ relpath, leafId, onRetry }: EditorViewProps) {
  const tabs = useEditorStore((s) => s.tabs)
  const setMode = useEditorStore((s) => s.setMode)
  const tabPos = useTabPosition(leafId)

  // Each leaf binds to exactly one file via its workspace state.relpath;
  // the leaf's own TabButton in `WorkspaceRenderer.TabStrip` is the
  // tab-switching UI now, so the editor plugin no longer draws its own
  // tab row on top of it.
  const activeTab = useMemo<EditorTab | null>(
    () => (relpath ? tabs.find((t) => t.relpath === relpath) ?? null : null),
    [tabs, relpath],
  )

  // Refs into the rendered body so an outline click can actually scroll
  // the right element. Preview uses the markdown body div; source uses
  // the CodeMirror view (via its imperative handle). Only one body is
  // mounted at a time.
  const markdownBodyRef = useRef<HTMLDivElement | null>(null)
  const cmViewRef = useRef<CodeMirrorHostHandle | null>(null)
  // The `overflow: auto` wrapper around the active tab body. In preview
  // mode this is the element whose scroll position drives heading
  // visibility (markdownBodyRef is the inner content). In source mode
  // the CodeMirror view owns its own scrolling.
  const scrollWrapRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    const unsub = eventBus.on<ScrollToHeadingPayload>('editor:scrollToHeading', (payload) => {
      if (!payload) return
      const tab = useEditorStore.getState().tabs.find(
        (t) => t.relpath === useEditorStore.getState().activeRelpath,
      )
      if (!tab) return
      if (tab.mode === 'preview') {
        // Preview: find the Nth heading in the rendered body. marked +
        // our parser agree on which lines are headings (both skip fenced
        // code), so `index` maps 1:1 to the Nth <h1..h6> in DOM order.
        const body = markdownBodyRef.current
        if (!body) return
        const headings = body.querySelectorAll<HTMLElement>('h1,h2,h3,h4,h5,h6')
        const target = headings[payload.index]
        if (!target) return
        target.scrollIntoView({ behavior: 'smooth', block: 'start' })
      } else {
        // Source / live: scroll the CM view so the target line lands
        // at the top. CM's doc.line is 1-based, matching our payload.
        const view = cmViewRef.current?.view ?? null
        if (!view) return
        viewToLine(view, payload.line)
      }
    })
    return unsub
  }, [])

  // Scroll-spy: report the topmost heading at/above the scroll
  // container's top edge so the outline can highlight it. The effect
  // re-binds on tab/mode change and on rendered-content changes (the
  // markdown body re-mounts via dangerouslySetInnerHTML; source-mode
  // line shifts can move headings). Compute is rAF-throttled, and we
  // emit only on transitions to avoid event spam.
  useEffect(() => {
    if (!activeTab || activeTab.loading || activeTab.error) return
    let raf = 0
    let lastIndex: number | null | undefined = undefined
    const emit = (idx: number | null) => {
      if (idx === lastIndex) return
      lastIndex = idx
      eventBus.emit(EVENT_ACTIVE_HEADING_CHANGED, { index: idx })
    }
    const compute = () => {
      raf = 0
      if (activeTab.mode === 'preview') {
        const wrap = scrollWrapRef.current
        const body = markdownBodyRef.current
        if (!wrap || !body) {
          emit(null)
          return
        }
        const headings = body.querySelectorAll<HTMLElement>('h1,h2,h3,h4,h5,h6')
        if (headings.length === 0) {
          emit(null)
          return
        }
        const wrapTop = wrap.getBoundingClientRect().top
        // Walk until we find the first heading still below the fold —
        // the previous one is the active section. If even the first is
        // still below, highlight it anyway: feels more grounded than
        // an unhighlighted outline at document top.
        let active = 0
        for (let i = 0; i < headings.length; i++) {
          const top = headings[i].getBoundingClientRect().top
          if (top <= wrapTop + ACTIVE_HEADING_OFFSET) active = i
          else break
        }
        emit(active)
      } else {
        // Source mode: the CM view owns scroll. Ask CM which line
        // corresponds to the scroll-container's top edge, then find
        // the heading at or above that. Headings come from the
        // outline store — same cross-plugin import pattern
        // outline/index.ts uses on the editor store.
        const view = cmViewRef.current?.view ?? null
        if (!view) {
          emit(null)
          return
        }
        const headings = useOutlineStore.getState().headings
        if (headings.length === 0) {
          emit(null)
          return
        }
        const scrollDom = view.scrollDOM
        const topY = scrollDom.getBoundingClientRect().top
        // `posAtCoords` resolves a viewport coordinate to a doc offset;
        // then `doc.lineAt` gives us the 1-based line. Falls back to
        // line 1 when CM can't resolve (not yet laid out, or element
        // has zero dimensions). The `side.top` TypeError is an internal
        // CM invariant failure — treat it the same as a null return.
        let pos: number | null = null
        try {
          pos = view.posAtCoords({ x: scrollDom.getBoundingClientRect().left + 1, y: topY + 1 })
        } catch {
          // view not laid out yet — fall through to line-1 default
        }
        const topLine = pos != null ? view.state.doc.lineAt(pos).number : 1
        let active = 0
        for (let i = 0; i < headings.length; i++) {
          if (headings[i].line <= topLine) active = i
          else break
        }
        emit(active)
      }
    }
    const schedule = () => {
      if (raf) return
      raf = requestAnimationFrame(compute)
    }
    const target =
      activeTab.mode === 'source' || activeTab.mode === 'live'
        ? cmViewRef.current?.view?.scrollDOM ?? null
        : scrollWrapRef.current
    if (!target) return
    target.addEventListener('scroll', schedule, { passive: true })
    // Initial compute after the next paint so the freshly-rendered DOM
    // (especially preview after innerHTML swap) has measurable layout.
    const initial = setTimeout(compute, 0)
    return () => {
      target.removeEventListener('scroll', schedule)
      clearTimeout(initial)
      if (raf) cancelAnimationFrame(raf)
    }
  }, [
    activeTab?.relpath,
    activeTab?.mode,
    activeTab?.loading,
    activeTab?.error,
    activeTab?.content,
  ])

  // Publish the currently-mounted CM view to the editor runtime so
  // the Find / Replace commands (registered in index.ts) can call
  // `openSearchPanel` on the active editor without taking a React
  // dependency. Source and live modes both mount a CM host; preview
  // is the only mode that clears the registration.
  useEffect(() => {
    if (!activeTab || activeTab.mode === 'preview') {
      setActiveCmView(null)
      return
    }
    const view = cmViewRef.current?.view ?? null
    setActiveCmView(view)
    return () => {
      setActiveCmView(null)
    }
  }, [
    activeTab?.relpath,
    activeTab?.mode,
    activeTab?.loading,
    activeTab?.error,
  ])

  // Parse markdown once per content change — re-running marked + DOMPurify
  // on every unrelated parent re-render would be needlessly expensive.
  const markdownHtml = useMemo(() => {
    if (!activeTab) return ''
    if (activeTab.loading || activeTab.error) return ''
    if (activeTab.mode !== 'preview') return ''
    if (!isMarkdown(activeTab.name)) return ''
    return renderMarkdown(activeTab.content)
  }, [
    activeTab?.relpath,
    activeTab?.content,
    activeTab?.name,
    activeTab?.loading,
    activeTab?.error,
    activeTab?.mode,
  ])

  // BL-008 — swap fenced-code placeholders for rendered widgets after the
  // sanitized markdown HTML has been mounted by React. Fired keyed on the
  // HTML string so a content edit re-runs hydration against the new tree.
  useEffect(() => {
    if (!markdownHtml) return
    hydrateFencedCode(markdownBodyRef.current)
  }, [markdownHtml])

  const rootStyle: React.CSSProperties = {
    display: 'flex',
    flexDirection: 'column',
    width: '100%',
    height: '100%',
    background: 'var(--background-primary)',
    color: 'var(--text-normal)',
    fontFamily: 'var(--font-interface)',
    fontSize: 'var(--ui-size, 13px)',
    overflow: 'hidden',
  }

  const centredStyle: React.CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '100%',
    height: '100%',
  }

  // The workspace-level TabStrip (WorkspaceRenderer.tsx) already sits
  // above this view and hosts the tab buttons + drag region space; no
  // per-column title bar is needed here.

  if (!activeTab) {
    return (
      <div style={rootStyle}>
        <ViewHeader activeTab={null} />
        <div style={centredStyle}>
          <EmptyStateActions hasAnyTab={tabs.length > 0} />
        </div>
      </div>
    )
  }

  return (
    <div style={rootStyle}>
      <ViewHeader
        activeTab={activeTab}
        mode={activeTab.mode}
        onToggleMode={() => {
          // Cycle live ↔ source. Reading-view (preview) is reachable
          // via the more-menu / command palette only — landing on
          // preview here would feel like a dead-end on a click that
          // the user expects to flip an editing surface.
          const next: EditorTabMode = activeTab.mode === 'source' ? 'live' : 'source'
          setMode(activeTab.relpath, next)
        }}
        onMoveLeft={
          tabPos && tabPos.index > 0
            ? () => workspace.reorderLeaves(tabPos.tabsId, tabPos.index, tabPos.index - 1)
            : undefined
        }
        onMoveRight={
          tabPos && tabPos.index < tabPos.total - 1
            ? () => workspace.reorderLeaves(tabPos.tabsId, tabPos.index, tabPos.index + 1)
            : undefined
        }
      />
      <div ref={scrollWrapRef} style={{ flex: '1 1 auto', overflow: 'auto', position: 'relative' }}>
        <TabBody
          tab={activeTab}
          markdownHtml={markdownHtml}
          onRetry={onRetry}
          markdownBodyRef={markdownBodyRef}
          cmViewRef={cmViewRef}
        />
      </div>
    </div>
  )
}

/**
 * Per-view header strip at the top of the editor area, mirroring
 * Obsidian's `.view-header` pattern. Three slots:
 *   left    — reserved for future back/forward navigation
 *   title   — breadcrumb over `activeTab.relpath`, final segment in
 *             --text-normal, earlier segments in --text-muted, separated by a
 *             right-chevron icon
 *   actions — reserved for future view-actions (e.g. pin, more menu)
 *
 * Always renders so the row height doesn't flicker in/out as tabs
 * open and close. With no active tab it shows a muted placeholder.
 * Untitled tabs (`untitled-N`) show just their tab name with no
 * path trail — they have no real directory hierarchy yet.
 */
interface ViewHeaderProps {
  activeTab: EditorTab | null
  mode?: EditorTabMode
  onToggleMode?: () => void
  onMoveLeft?: () => void
  onMoveRight?: () => void
}

function ViewHeader({ activeTab, mode, onToggleMode, onMoveLeft, onMoveRight }: ViewHeaderProps) {
  const moreButtonRef = useRef<HTMLButtonElement | null>(null)
  const [moreOpen, setMoreOpen] = useState(false)
  const moreAnchorRect = moreOpen
    ? moreButtonRef.current?.getBoundingClientRect() ?? null
    : null

  const isUntitledRel = activeTab ? /^untitled-\d+$/i.test(activeTab.relpath) : false
  const moreItems = useMemo(
    () =>
      activeTab
        ? buildTabContextMenu({ mode: activeTab.mode, isUntitled: isUntitledRel })
        : [],
    [activeTab?.mode, activeTab?.relpath, isUntitledRel],
  )

  const disabledNavStyle: React.CSSProperties = {
    background: 'transparent',
    border: 'none',
    color: 'var(--text-muted)',
    cursor: 'default',
    opacity: 0.45,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: 28,
    height: 28,
    borderRadius: 4,
  }
  const activeNavStyle: React.CSSProperties = {
    background: 'transparent',
    border: 'none',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: 28,
    height: 28,
    borderRadius: 4,
  }
  return (
    <div className="view-header">
      <div className="view-header-left" style={{ display: 'flex', gap: 2, alignItems: 'center' }}>
        <button
          type="button"
          aria-label="Move tab left"
          title="Move tab left"
          disabled={!onMoveLeft}
          onClick={onMoveLeft}
          style={onMoveLeft ? activeNavStyle : disabledNavStyle}
        >
          <Icon name="arrowLeft" size={16} />
        </button>
        <button
          type="button"
          aria-label="Move tab right"
          title="Move tab right"
          disabled={!onMoveRight}
          onClick={onMoveRight}
          style={onMoveRight ? activeNavStyle : disabledNavStyle}
        >
          <Icon name="arrowRight" size={16} />
        </button>
      </div>
      <div className="view-header-title-container">
        <div className="view-header-title-parent">
          <BreadcrumbSegments activeTab={activeTab} />
        </div>
      </div>
      <div className="view-actions" style={{ display: 'flex', gap: 2, alignItems: 'center' }}>
        {mode && onToggleMode && (
          <ModeToggle mode={mode} onClick={onToggleMode} />
        )}
        <button
          ref={moreButtonRef}
          type="button"
          aria-label="More options"
          title="More options"
          aria-expanded={moreOpen}
          disabled={!activeTab}
          onClick={() => {
            if (!activeTab) return
            setMoreOpen((v) => !v)
          }}
          style={activeTab ? activeNavStyle : disabledNavStyle}
        >
          <Icon name="more" size={16} />
        </button>
        <ContextMenu
          open={moreOpen}
          anchorRect={moreAnchorRect}
          items={moreItems}
          onClose={() => setMoreOpen(false)}
        />
      </div>
    </div>
  )
}

function BreadcrumbSegments({ activeTab }: { activeTab: EditorTab | null }) {
  if (!activeTab) {
    return (
      <span className="view-header-title" style={{ color: 'var(--text-faint)' }}>
        No file open
      </span>
    )
  }

  // Untitled tabs have no path hierarchy — render the bare name.
  if (isUntitledRelpath(activeTab.relpath)) {
    return <span className="view-header-title">{activeTab.name}</span>
  }

  const segments = splitPathSegments(activeTab.relpath)
  if (segments.length === 0) {
    return <span className="view-header-title">{activeTab.name}</span>
  }

  const lastIndex = segments.length - 1
  return (
    <>
      {segments.slice(0, -1).map((seg, i) => (
        <span key={i} style={{ display: 'inline-flex', alignItems: 'center' }}>
          <span className="view-header-breadcrumb">{seg}</span>
          <span className="view-header-breadcrumb-separator" aria-hidden>
            <Icon name="chev" size={12} />
          </span>
        </span>
      ))}
      <span className="view-header-title">{segments[lastIndex]}</span>
    </>
  )
}

/**
 * Obsidian-style action-link stack shown when the editor pane has no
 * active tab. Three links: create new note, open command palette
 * ("Go to file"), and close the current tab. The close link is only
 * shown when there's at least one tab in the store — otherwise there's
 * nothing to close and the link would be a dead end.
 *
 * Each link is styled as an inline text-button using existing CSS
 * tokens (no new colours). Keybinding hints are resolved from the
 * live `KeybindingRegistry` via `findByCommand`, so a user override
 * flows through to the pill text without a reload; a missing binding
 * falls back to the documented default so the hint still appears on
 * a minimal plugin set.
 */
export function EmptyStateActions({ hasAnyTab }: { hasAnyTab: boolean }) {
  // `getRegistry()` is a synchronous reference read — if the shell
  // finishes booting after this component mounts (unlikely; the empty
  // state only renders once the workspace has hydrated), the fallback
  // strings cover the gap until the next render.
  const chordFor = (commandId: string, fallback: string): string => {
    const reg = getRegistry()
    return reg?.keybindings.formattedChordFor(commandId) ?? fallback
  }
  const linkStyle: React.CSSProperties = {
    background: 'transparent',
    border: 0,
    padding: '4px 8px',
    color: 'var(--interactive-accent)',
    cursor: 'pointer',
    fontFamily: 'inherit',
    fontSize: 'inherit',
    textAlign: 'center',
    borderRadius: 'var(--radius-s, 4px)',
  }
  const hintStyle: React.CSSProperties = {
    color: 'var(--text-muted)',
    marginLeft: 4,
  }

  const runCommand = (commandId: string) => {
    const reg = getRegistry()
    if (!reg) return
    void reg.commands.execute(commandId)
  }

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: 6,
      }}
    >
      <button
        type="button"
        style={linkStyle}
        onMouseEnter={(e) => {
          (e.currentTarget as HTMLButtonElement).style.textDecoration = 'underline'
        }}
        onMouseLeave={(e) => {
          (e.currentTarget as HTMLButtonElement).style.textDecoration = 'none'
        }}
        onClick={() => runCommand('nexus.editor.newUntitled')}
      >
        Create new note<span style={hintStyle}>({chordFor('nexus.editor.newUntitled', 'Ctrl + N')})</span>
      </button>
      <button
        type="button"
        style={linkStyle}
        onMouseEnter={(e) => {
          (e.currentTarget as HTMLButtonElement).style.textDecoration = 'underline'
        }}
        onMouseLeave={(e) => {
          (e.currentTarget as HTMLButtonElement).style.textDecoration = 'none'
        }}
        onClick={() => runCommand('nexus.commandPalette.open')}
      >
        Go to file<span style={hintStyle}>({chordFor('nexus.commandPalette.open', 'Ctrl + O')})</span>
      </button>
      {hasAnyTab && (
        <button
          type="button"
          style={linkStyle}
          onMouseEnter={(e) => {
            (e.currentTarget as HTMLButtonElement).style.textDecoration = 'underline'
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLButtonElement).style.textDecoration = 'none'
          }}
          onClick={() => runCommand('nexus.editor.closeTab')}
        >
          Close
        </button>
      )}
    </div>
  )
}

interface ModeToggleProps {
  mode: EditorTabMode
  onClick: () => void
}

/**
 * Right-edge mode toggle.
 *   - `live`    → pencil (click → source).
 *   - `source`  → eye (click → live, i.e. WYSIWYG preview).
 *   - `preview` → pencil (click → live; "back to edit").
 * Aria-label mirrors the action.
 */
function ModeToggle({ mode, onClick }: ModeToggleProps) {
  const showPencil = mode === 'live' || mode === 'preview'
  const label =
    mode === 'live' ? 'Edit source'
    : mode === 'source' ? 'Live preview'
    : 'Edit'

  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = 'var(--background-modifier-hover)'
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = 'transparent'
      }}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        flex: '0 0 32px',
        width: 32,
        height: 32,
        alignSelf: 'center',
        marginRight: 4,
        padding: 0,
        border: 0,
        background: 'transparent',
        color: 'var(--text-muted)',
        cursor: 'pointer',
        borderRadius: 'var(--radius-s)',
      }}
    >
      {showPencil ? (
        <svg
          width={16}
          height={16}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.75}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M12 20 h9 M16.5 3.5 a2.12 2.12 0 0 1 3 3 L7 19 l-4 1 1 -4 z" />
        </svg>
      ) : (
        <svg
          width={16}
          height={16}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.75}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M2 12 s3.5 -7 10 -7 s10 7 10 7 s-3.5 7 -10 7 s-10 -7 -10 -7 z" />
          <circle cx={12} cy={12} r={3} />
        </svg>
      )}
    </button>
  )
}

interface TabBodyProps {
  tab: EditorTab
  markdownHtml: string
  onRetry: (relpath: string) => void
  markdownBodyRef: React.MutableRefObject<HTMLDivElement | null>
  cmViewRef: React.MutableRefObject<CodeMirrorHostHandle | null>
}

function TabBody({ tab, markdownHtml, onRetry, markdownBodyRef, cmViewRef }: TabBodyProps) {
  const centredStyle: React.CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '100%',
    height: '100%',
  }

  if (tab.error) {
    return (
      <div style={centredStyle}>
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12 }}>
          <div style={{ color: 'var(--risk)', maxWidth: 480, textAlign: 'center' }}>
            {tab.error}
          </div>
          <button
            onClick={() => onRetry(tab.relpath)}
            style={{
              background: 'var(--background-secondary)',
              color: 'var(--text-normal)',
              border: '1px solid var(--divider-color)',
              borderRadius: 'var(--radius-s, 6px)',
              padding: '6px 14px',
              fontFamily: 'var(--font-interface)',
              fontSize: 'var(--ui-size, 13px)',
              cursor: 'pointer',
            }}
          >
            Retry
          </button>
        </div>
      </div>
    )
  }

  if (tab.loading) {
    return <div style={{ ...centredStyle, color: 'var(--text-faint)' }}>Loading…</div>
  }

  // HTML files render in a sandboxed iframe in live/preview mode so the
  // document's own styles and structure are visible. `sandbox=""` blocks
  // scripts, forms, popups, and top navigation; the file is treated as a
  // unique origin. Source mode falls through to CM6 for raw editing.
  if (tab.mode !== 'source' && isHtml(tab.name)) {
    return (
      <iframe
        key={`html:${tab.relpath}`}
        title={tab.name}
        srcDoc={withHtmlViewerOverrides(tab.content)}
        sandbox=""
        style={{ width: '100%', height: '100%', border: 0 }}
      />
    )
  }

  if (tab.mode === 'source' || tab.mode === 'live') {
    // Phase 5: markdown tabs with an open kernel session route their
    // edits through `com.nexus.editor::apply_transaction` via the
    // bridge — `onChange` becomes a no-op for the hot path and the
    // authoritative snapshot drives the doc. Untitled tabs (no
    // session) keep the Phase 2 behaviour: `setContent` mutates the
    // store directly so the local buffer stays live until first save.
    const runtime = getEditorRuntime()
    const bridgeEligible =
      runtime !== null &&
      !isUntitled(tab.relpath) &&
      isMarkdown(tab.name) &&
      runtime.sessionManager.refcount(tab.relpath) > 0

    // `key` prefix differs per-mode so toggling source ↔ live cleanly
    // remounts CodeMirrorHost rather than trying to reconcile the
    // extension list in place. BL-070: include the active keybinding
    // layer so a settings flip remounts the host with the new keymap
    // (the vim layer's modal state can't be hot-swapped in place).
    const keybindings = runtime?.getKeybindings() ?? 'default'
    const keyPrefix = tab.mode === 'live' ? 'live' : (bridgeEligible ? 'bridge' : 'local')
    const keymapKey = `${keyPrefix}:${keybindings}`

    if (bridgeEligible && runtime) {
      return (
        <CodeMirrorHost
          key={`${keymapKey}:${tab.relpath}`}
          ref={cmViewRef}
          className="nexus-editor-source"
          value={tab.content}
          onChange={() => {
            // No-op on the hot path. The bridge owns dispatching the
            // edit through the kernel; the store's `content` is
            // reconciled lazily when a snapshot update lands.
          }}
          keybindings={keybindings}
          vim={
            keybindings === 'vim'
              ? {
                  relpath: tab.relpath,
                  onSave: () => {
                    void runtime.kernelClient.saveSession(tab.relpath)
                  },
                  onClose: () => {
                    void runtime.confirmAndClose(tab.relpath)
                  },
                }
              : undefined
          }
          emacs={
            keybindings === 'emacs' ? { relpath: tab.relpath } : undefined
          }
          kernelUndo={{
            relpath: tab.relpath,
            kernelClient: runtime.kernelClient,
            applyCanonical: (view, canonical) => {
              const current = view.state.doc.toString()
              if (current === canonical) return
              view.dispatch({
                changes: { from: 0, to: current.length, insert: canonical },
              })
            },
            onError: runtime.reportBridgeError,
          }}
          buildExtensions={() => {
            const base = [
              transactionBridge({
                relpath: tab.relpath,
                kernelClient: runtime.kernelClient,
                getSnapshot: () => runtime.sessionManager.getSnapshot(tab.relpath),
                onError: runtime.reportBridgeError,
              }),
              slashCommandExt(),
              blockSelectionExt(),
              multiCursorPromoteExt(),
              blockHandleExt(),
              inputRulesExt(),
              inlineToolbarExt(),
              ghostCompletionExt(),
              linkSuggestExt(),
              marginSuggestionsExt({ relpath: tab.relpath }),
              marginSuggestTriggerExt({ relpath: tab.relpath }),
            ]
            return tab.mode === 'live'
              ? [
                  ...base,
                  livePreviewExt(),
                  databaseViewExt({
                    client: runtime.kernelClient,
                    onError: runtime.reportBridgeError,
                    events: runtime.kernelEvents,
                  }),
                  ...(runtime.onBlockLinkNavigate
                    ? [blockLinkNavExt({ onNavigate: runtime.onBlockLinkNavigate })]
                    : []),
                ]
              : base
          }}
        />
      )
    }

    // BL-075 — code mode: non-markdown files routed through the
    // dual-mode router get a CodeMirror with the matching language
    // extension and *no* block-tree extensions (no slash menu, no
    // block handles, no live-preview decorations). Document mode
    // for non-markdown / pre-session files (untitled, plain text)
    // keeps the Phase-2 fallback shape: bare CM6 with no language.
    //
    // The `codeFileExtensions` list comes from the runtime (which
    // reads the live `nexus.editor.codeFileExtensions` setting), so
    // a user adding `.sh` to the list opens that file in code mode
    // on the next reopen.
    const codeExtensions = runtime?.getCodeFileExtensions()
    const editorMode = getEditorMode(tab.name, codeExtensions)
    const languageExtension =
      editorMode === 'code' ? pickLanguageExtension(tab.name) : null
    const codeBuildExtensions =
      languageExtension !== null
        ? () => [languageExtension]
        : tab.mode === 'live'
          ? () => [livePreviewExt()]
          : undefined

    // Untitled / non-markdown / pre-session fallback. The
    // `buildExtensions` choice above turns this into either:
    //   - bare CM6 (no language, document fallback for unrecognised
    //     names like `LICENSE`),
    //   - CM6 + livePreviewExt (fallback for live mode on a tab
    //     with no kernel session — preserves Phase-2 behaviour),
    //   - CM6 + language extension (BL-075 code mode).
    return (
      <CodeMirrorHost
        key={`${keymapKey}:${editorMode}:${tab.relpath}`}
        ref={cmViewRef}
        className="nexus-editor-source"
        value={tab.content}
        onChange={(v) => useEditorStore.getState().setContent(tab.relpath, v)}
        keybindings={keybindings}
        vim={
          keybindings === 'vim' && runtime
            ? {
                relpath: tab.relpath,
                // No session → save is a no-op; close still routes
                // through the runtime's confirmation flow. Code
                // mode files have a save (storage::write_file via
                // the COMMAND_SAVE handler), so the vim `:w` plumb
                // could route through the runtime — but the
                // existing runtime.confirmAndClose path doesn't yet
                // expose a save-by-relpath, and the COMMAND_SAVE
                // handler reads the active tab anyway. ⌘S works as
                // expected; only the ex-command :w is a no-op,
                // matching pre-BL-075 behaviour.
                onSave: () => {},
                onClose: () => {
                  void runtime.confirmAndClose(tab.relpath)
                },
              }
            : undefined
        }
        emacs={
          keybindings === 'emacs' ? { relpath: tab.relpath } : undefined
        }
        buildExtensions={codeBuildExtensions}
      />
    )
  }

  if (isMarkdown(tab.name)) {
    return (
      <div
        ref={markdownBodyRef}
        className="nexus-markdown-body"
        dangerouslySetInnerHTML={{ __html: markdownHtml }}
      />
    )
  }

  return <pre className="nexus-editor-raw">{tab.content}</pre>
}
