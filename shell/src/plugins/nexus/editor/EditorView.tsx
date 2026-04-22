import { useEffect, useMemo, useRef } from 'react'
import { EditorView as CMEditorView } from '@codemirror/view'
import { useEditorStore, type EditorTab, type EditorTabMode } from './editorStore'
import { renderMarkdown } from './markdownRender'
import { eventBus } from '../../../host/EventBus'
import { useOutlineStore } from '../outline/outlineStore'
import { Icon } from '../../../icons'
import { useWorkspaceField, workspace } from '../../../workspace'
import { getEditorRuntime } from './runtime'
import { CodeMirrorHost, type CodeMirrorHostHandle } from './cm/CodeMirrorHost'
import { transactionBridge } from './cm/transactionBridge'
import { getRegistry } from '../../../host/shellRegistry'
import './markdown.css'

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
  onRetry: (relpath: string) => void
}

function isMarkdown(name: string): boolean {
  return /\.(md|markdown|mdx)$/i.test(name)
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
export function EditorView({ relpath, onRetry }: EditorViewProps) {
  const tabs = useEditorStore((s) => s.tabs)
  const setMode = useEditorStore((s) => s.setMode)

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
      } else if (tab.mode === 'source') {
        // Source: scroll the CM view so the target line lands at the top.
        // CM's doc.line is 1-based, matching our payload.
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
        // then `doc.lineAt` gives us the 1-based line. Fall back to
        // line 1 if CM can't resolve (e.g. view hasn't laid out yet).
        const pos = view.posAtCoords({ x: scrollDom.getBoundingClientRect().left + 1, y: topY + 1 })
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
      activeTab.mode === 'preview'
        ? scrollWrapRef.current
        : cmViewRef.current?.view?.scrollDOM ?? null
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

  const rootStyle: React.CSSProperties = {
    display: 'flex',
    flexDirection: 'column',
    width: '100%',
    height: '100%',
    background: 'var(--bg)',
    color: 'var(--fg)',
    fontFamily: 'var(--f-ui)',
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
      <ViewHeader activeTab={activeTab} />
      <div ref={scrollWrapRef} style={{ flex: '1 1 auto', overflow: 'auto', position: 'relative' }}>
        <div
          style={{
            position: 'absolute',
            top: 8,
            right: 12,
            zIndex: 2,
          }}
        >
          <ModeToggle
            mode={activeTab.mode}
            onClick={() => {
              const next: EditorTabMode = activeTab.mode === 'preview' ? 'source' : 'preview'
              setMode(activeTab.relpath, next)
            }}
          />
        </div>
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
 *             --fg, earlier segments in --fg-muted, separated by a
 *             right-chevron icon
 *   actions — reserved for future view-actions (e.g. pin, more menu)
 *
 * Always renders so the row height doesn't flicker in/out as tabs
 * open and close. With no active tab it shows a muted placeholder.
 * Untitled tabs (`untitled-N`) show just their tab name with no
 * path trail — they have no real directory hierarchy yet.
 */
function ViewHeader({ activeTab }: { activeTab: EditorTab | null }) {
  return (
    <div className="view-header">
      <div className="view-header-left" />
      <div className="view-header-title-container">
        <div className="view-header-title-parent">
          <BreadcrumbSegments activeTab={activeTab} />
        </div>
      </div>
      <div className="view-actions" />
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
 * tokens (no new colours). Keybinding hints use documented defaults;
 * the KeybindingRegistry doesn't expose a lookup-by-command helper
 * today, so we can't resolve the live binding at render time.
 * TODO: source keybinding hints from the KeybindingRegistry once it
 * gains a `findByCommand` method.
 */
export function EmptyStateActions({ hasAnyTab }: { hasAnyTab: boolean }) {
  const linkStyle: React.CSSProperties = {
    background: 'transparent',
    border: 0,
    padding: '4px 8px',
    color: 'var(--accent)',
    cursor: 'pointer',
    fontFamily: 'inherit',
    fontSize: 'inherit',
    textAlign: 'center',
    borderRadius: 'var(--r, 4px)',
  }
  const hintStyle: React.CSSProperties = {
    color: 'var(--fg-muted)',
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
        Create new note<span style={hintStyle}>(Ctrl + N)</span>
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
        Go to file<span style={hintStyle}>(Ctrl + O)</span>
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
 * Right-edge mode toggle. Shows the icon for the action the click
 * will perform: pencil when in preview (click to edit), eye when in
 * source (click to preview). Aria-label mirrors the action.
 */
function ModeToggle({ mode, onClick }: ModeToggleProps) {
  const willEdit = mode === 'preview'
  const label = willEdit ? 'Edit' : 'Preview'

  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-hover)'
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
        color: 'var(--fg-muted)',
        cursor: 'pointer',
        borderRadius: 'var(--r)',
      }}
    >
      {willEdit ? (
        // Pencil — click to edit (currently in preview)
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
        // Eye — click to preview (currently in source)
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
              background: 'var(--bg-raised)',
              color: 'var(--fg)',
              border: '1px solid var(--line-soft)',
              borderRadius: 'var(--r, 6px)',
              padding: '6px 14px',
              fontFamily: 'var(--f-ui)',
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
    return <div style={{ ...centredStyle, color: 'var(--fg-dim)' }}>Loading…</div>
  }

  if (tab.mode === 'source') {
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

    if (bridgeEligible && runtime) {
      return (
        <CodeMirrorHost
          key={`bridge:${tab.relpath}`}
          ref={cmViewRef}
          className="nexus-editor-source"
          value={tab.content}
          onChange={() => {
            // No-op on the hot path. The bridge owns dispatching the
            // edit through the kernel; the store's `content` is
            // reconciled lazily when a snapshot update lands.
          }}
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
          buildExtensions={() => [
            transactionBridge({
              relpath: tab.relpath,
              kernelClient: runtime.kernelClient,
              getSnapshot: () => runtime.sessionManager.getSnapshot(tab.relpath),
              onError: runtime.reportBridgeError,
            }),
          ]}
        />
      )
    }

    // Untitled / non-markdown / pre-session fallback — Phase 2 behaviour.
    return (
      <CodeMirrorHost
        key={`local:${tab.relpath}`}
        ref={cmViewRef}
        className="nexus-editor-source"
        value={tab.content}
        onChange={(v) => useEditorStore.getState().setContent(tab.relpath, v)}
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
