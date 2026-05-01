// React render layer for the Leaf + ViewRegistry workspace model.
// Plan reference: /home/baileyrd/projects/nexus/docs/leaf-migration-plan.md §Phase 4.
//
// Design notes:
// - Tree nodes are mutated IN PLACE by the store; top-level Zustand state
//   identity does not change on tree edits. We force re-render by
//   subscribing to the `layout-change` event (see `useLayoutVersion`).
// - `<LeafHost>` is the one place a View's DOM lives. The wrapper div has
//   no React children — there is nothing for React to diff inside. Tab
//   switches toggle `display: none`, leaving mounted DOM intact so
//   switching back is instant (plan line 134).
// - Floating windows are rendered inline with `data-floating="true"` for
//   Phase 4 scope. Tauri multi-window comes later.
import React, {
  memo,
  useEffect,
  useReducer,
  useRef,
  useState,
  type CSSProperties,
} from 'react'
import { Icon } from '../icons/index.tsx'
import { WindowControls } from '../shell/WindowControls'
import { zIndex } from '../shell/zIndex'
import type {
  FloatingWindow as FloatingWindowNode,
  Leaf,
  Root,
  Sidedock,
  Split,
  Tabs,
  WorkspaceParent,
} from './types.ts'
import { workspace } from './workspaceStore.ts'
import { popoutLeaf as popoutLeafBridge } from './popoutWindowBridge.ts'
import { ForgeSelector } from './ForgeSelector.tsx'
import { RightPanelFooter } from './RightPanelFooter.tsx'

// ---------------------------------------------------------------------------
// Layout-change subscription hook.
//
// Tree mutations are in-place; object identity of `workspace.rootSplit` etc.
// does NOT change. We bump a local counter on every `layout-change` so the
// render tree re-runs. One hook per top-level component is plenty — inner
// renderers flow from props, which read the mutated tree on every render.
// ---------------------------------------------------------------------------

function useLayoutVersion(): void {
  const [, force] = useReducer((x: number) => x + 1, 0)
  useEffect(() => {
    const off = workspace.on('layout-change', () => force())
    return off
  }, [])
}

// ---------------------------------------------------------------------------
// <Workspace> — top-level. Renders left dock + main area + right dock.
// ---------------------------------------------------------------------------

// Outer container: stacks a horizontal row (left|main|right) on top of
// the bottom drawer so the drawer spans the full window width, matching
// Obsidian's bottom pane / VS Code's integrated-terminal drawer layout.
const ROOT_STYLE: CSSProperties = {
  position: 'relative',
  display: 'flex',
  flexDirection: 'column',
  width: '100%',
  height: '100%',
  minWidth: 0,
  minHeight: 0,
  overflow: 'hidden',
}

const UPPER_ROW_STYLE: CSSProperties = {
  display: 'flex',
  flexDirection: 'row',
  flex: '1 1 auto',
  minWidth: 0,
  minHeight: 0,
  overflow: 'hidden',
}

const MAIN_STYLE: CSSProperties = {
  flex: '1 1 auto',
  display: 'flex',
  flexDirection: 'column',
  minWidth: 0,
  minHeight: 0,
  overflow: 'hidden',
}

const MAIN_CONTENT_STYLE: CSSProperties = {
  flex: '1 1 auto',
  display: 'flex',
  minWidth: 0,
  minHeight: 0,
  overflow: 'hidden',
}

export function Workspace(): JSX.Element {
  useLayoutVersion()
  const rootSplit = workspace.rootSplit
  const leftSplit = workspace.leftSplit
  const rightSplit = workspace.rightSplit
  const bottomSplit = workspace.bottomSplit
  const floating = workspace.floating

  return (
    <div className="workspace-root" style={ROOT_STYLE}>
      <div className="workspace-upper" style={UPPER_ROW_STYLE}>
        <SidedockFrame side="left" dock={leftSplit} />
        <div className="workspace-main" style={MAIN_STYLE}>
          <div className="workspace-main-content" style={MAIN_CONTENT_STYLE}>
            <RenderNode node={rootSplit} isMainDock />
          </div>
          <SidedockFrame side="bottom" dock={bottomSplit} />
        </div>
        <SidedockFrame side="right" dock={rightSplit} />
      </div>
      {/* Floating window-controls anchor. Absolutely positioned at the
          window's top-right corner so it sits over whichever view /
          panel happens to render there, without introducing a new
          title-bar row that would stack beneath the native chrome. */}
      <div
        style={{
          position: 'absolute',
          top: 0,
          right: 0,
          zIndex: zIndex.chromeControls,
          pointerEvents: 'auto',
        }}
      >
        <WindowControls />
      </div>
      {floating.map((fw) => (
        <div
          key={fw.id}
          data-floating="true"
          style={{ display: 'none' }}
          aria-hidden="true"
        >
          <RenderNode node={fw} />
        </div>
      ))}
    </div>
  )
}

// ---------------------------------------------------------------------------
// <SidedockFrame> — a Sidedock plus collapse button + resize handle.
// ---------------------------------------------------------------------------

const COLLAPSE_THRESHOLD = 120
const DOCK_MIN_SIZE = 150
const RIBBON_WIDTH = 24
// Height of the collapsed bottom-drawer strip. Roughly matches a single
// tab row — enough to host a label + expand button without eating space.
const BOTTOM_COLLAPSED_HEIGHT = 28

type DockSide = 'left' | 'right' | 'bottom'

interface SidedockFrameProps {
  side: DockSide
  dock: Sidedock
}

const COLLAPSED_BAR_STYLE: CSSProperties = {
  width: RIBBON_WIDTH,
  flex: `0 0 ${RIBBON_WIDTH}px`,
  display: 'flex',
  alignItems: 'flex-start',
  justifyContent: 'center',
  background: 'var(--background-secondary, #252526)',
  borderRight: '1px solid var(--divider-color, var(--background-modifier-border, #333))',
}

function SidedockFrame({ side, dock }: SidedockFrameProps): JSX.Element {
  if (side === 'bottom') return <BottomSidedockFrame dock={dock} />

  if (dock.collapsed) {
    // Both sidedocks are fully hidden when collapsed. Re-expand
    // affordances:
    //   - left:  activity bar's sidebar-toggle button (top-left)
    //   - right: PanelRight icon in the main dock's tab strip
    return <></>
  }

  const panel = (
    <div
      className={`workspace-sidedock mod-${side}`}
      style={{
        flex: `0 0 ${dock.size}px`,
        width: dock.size,
        minWidth: DOCK_MIN_SIZE,
        display: 'flex',
        flexDirection: 'column',
        background: 'var(--background-secondary, #252526)',
        overflow: 'hidden',
      }}
    >
      {/* Collapse chevron now lives inside the TabStrip (threaded via
          sideDock prop) so left/center/right columns all have a single
          36px header row that lines up with WindowControls. */}
      <div style={{ flex: '1 1 auto', minHeight: 0, display: 'flex' }}>
        <RenderNode node={dock} sideDock={side} />
      </div>
      {side === 'left' && <ForgeSelector />}
      {side === 'right' && <RightPanelFooter />}
    </div>
  )

  const handle = (
    <DockResizeHandle
      key={`handle-${side}`}
      side={side}
      initialSize={dock.size}
    />
  )

  // Handle placement: inner edge. Left dock -> handle right of panel;
  // right dock -> handle left of panel.
  return side === 'left' ? (
    <>
      {panel}
      {handle}
    </>
  ) : (
    <>
      {handle}
      {panel}
    </>
  )
}

// ---------------------------------------------------------------------------
// <BottomSidedockFrame> — horizontal drawer spanning the full window width.
// Collapse state shows a 28-px strip with an expand button; expanded state
// shows the tabs group with a top-edge resize handle.
// ---------------------------------------------------------------------------

function BottomSidedockFrame({ dock }: { dock: Sidedock }): JSX.Element {
  // Bottom drawer fully hidden when collapsed — same treatment as the
  // side docks. Re-expand via a terminal / bottom-drawer toggle command
  // or keybinding; no persistent strip.
  if (dock.collapsed) return <></>


  return (
    <div
      className="workspace-sidedock mod-bottom"
      style={{
        flex: `0 0 ${dock.size}px`,
        height: dock.size,
        minHeight: DOCK_MIN_SIZE,
        display: 'flex',
        flexDirection: 'column',
        background: 'var(--background-secondary, #252526)',
        overflow: 'hidden',
      }}
    >
      {/* Top-edge resize handle: dragging up grows the drawer. */}
      <DockResizeHandle side="bottom" initialSize={dock.size} />
      <div style={{ flex: '1 1 auto', minHeight: 0, display: 'flex' }}>
        <RenderNode node={dock} hideTabStrip />
      </div>
    </div>
  )
}

// ---------------------------------------------------------------------------
// DockResizeHandle — local variant. ResizeHandle.tsx measures
// `previousElementSibling`; it only fits when the handle sits to the right
// of the panel. Our right dock reverses that, so we implement a small
// side-aware handle rather than force-fit the shared one.
// ---------------------------------------------------------------------------

interface DockResizeHandleProps {
  side: DockSide
  initialSize: number
}

function DockResizeHandle({ side, initialSize }: DockResizeHandleProps): JSX.Element {
  const startPos = useRef(0)
  const startSize = useRef(initialSize)
  const horizontal = side === 'bottom'

  const onMouseDown = (e: React.MouseEvent): void => {
    e.preventDefault()
    startPos.current = horizontal ? e.clientY : e.clientX
    startSize.current = initialSize

    const onMouseMove = (ev: MouseEvent): void => {
      const cur = horizontal ? ev.clientY : ev.clientX
      const delta = cur - startPos.current
      // Left dock grows on +X delta; right dock grows on -X delta;
      // bottom dock grows on -Y delta (dragging up expands height).
      let target: number
      if (side === 'left') target = startSize.current + delta
      else if (side === 'right') target = startSize.current - delta
      else target = startSize.current - delta // bottom
      if (target < COLLAPSE_THRESHOLD) {
        workspace.setSidedockCollapsed(side, true)
        cleanup()
        return
      }
      workspace.setSidedockSize(side, target)
    }

    const cleanup = (): void => {
      document.removeEventListener('mousemove', onMouseMove)
      document.removeEventListener('mouseup', cleanup)
    }

    document.addEventListener('mousemove', onMouseMove)
    document.addEventListener('mouseup', cleanup)
  }

  return (
    <div
      className="workspace-dock-resize-handle"
      onMouseDown={onMouseDown}
      style={
        horizontal
          ? {
              flex: '0 0 2px',
              height: 2,
              width: '100%',
              cursor: 'row-resize',
              background: 'transparent',
              zIndex: 1,
            }
          : {
              flex: '0 0 2px',
              width: 2,
              cursor: 'col-resize',
              background: 'transparent',
              zIndex: 1,
            }
      }
    />
  )
}

// ---------------------------------------------------------------------------
// <RenderNode> — pure switch on node.kind. No own state.
// ---------------------------------------------------------------------------

interface RenderNodeProps {
  node: WorkspaceParent
  /** True when this subtree is descended from the main (rootSplit) dock.
   *  Used to gate the trailing "+ new tab" button on tab strips — that
   *  affordance is main-dock only, matching Obsidian. Sidedocks and
   *  floating windows don't get it. */
  isMainDock?: boolean
  /** Set when this subtree is the left or right sidedock. Threaded down
   *  so the TabStrip can render the sidebar collapse chevron inline
   *  with its tabs (rather than a separate stacked row above). */
  sideDock?: 'left' | 'right'
  /** Suppress rendering of the tab strip on any descendant tab groups.
   *  Used by the bottom drawer to render a chrome-free full-bleed view. */
  hideTabStrip?: boolean
}

function RenderNode({ node, isMainDock = false, sideDock, hideTabStrip }: RenderNodeProps): JSX.Element | null {
  switch (node.kind) {
    case 'split':
      return <SplitNode node={node} isMainDock={isMainDock} sideDock={sideDock} hideTabStrip={hideTabStrip} />
    case 'tabs':
      return <TabGroup tabs={node} isMainDock={isMainDock} sideDock={sideDock} hideTabStrip={hideTabStrip} />
    case 'root':
      return <RenderNode node={(node as Root).child} isMainDock={isMainDock} sideDock={sideDock} hideTabStrip={hideTabStrip} />
    case 'floating':
      return (
        <div data-floating="true" style={{ width: '100%', height: '100%' }}>
          <RenderNode node={(node as FloatingWindowNode).child} isMainDock={false} />
        </div>
      )
    default: {
      // Exhaustiveness guard. Unreachable at runtime; type model covers all variants.
      const _never: never = node
      void _never
      return null
    }
  }
}

function SplitNode({ node, isMainDock, sideDock, hideTabStrip }: { node: Split; isMainDock: boolean; sideDock?: 'left' | 'right'; hideTabStrip?: boolean }): JSX.Element {
  // OI-02 — per-split child DOM refs so the resize handle can measure
  // the live rects at drag start (first-drag initialisation snaps the
  // current flex proportions into concrete numbers, after which the
  // store carries the sizes on every subsequent drag + reload).
  const childRefs = useRef<(HTMLDivElement | null)[]>([])

  const horizontal = node.direction === 'horizontal'
  const style: CSSProperties = {
    display: 'flex',
    flexDirection: horizontal ? 'row' : 'column',
    flex: '1 1 auto',
    minWidth: 0,
    minHeight: 0,
    width: '100%',
    height: '100%',
  }

  return (
    <div className="workspace-split" style={style}>
      {node.children.map((child, i) => {
        const childFlex = node.sizes?.[i] ?? 1
        return (
          <React.Fragment key={childKey(child)}>
            <div
              ref={(el) => {
                childRefs.current[i] = el
              }}
              style={{
                flex: `${childFlex} ${childFlex} 0`,
                minWidth: 0,
                minHeight: 0,
                display: 'flex',
              }}
            >
              <RenderNode node={child} isMainDock={isMainDock} sideDock={sideDock} hideTabStrip={hideTabStrip} />
            </div>
            {i < node.children.length - 1 && (
              <SplitResizeHandle
                splitId={node.id}
                boundaryIndex={i}
                horizontal={horizontal}
                childRefs={childRefs}
                currentSizes={node.sizes}
              />
            )}
          </React.Fragment>
        )
      })}
    </div>
  )
}

// ---------------------------------------------------------------------------
// <SplitResizeHandle> (OI-02) — drag divider between two SplitNode children.
//
// Drag math:
//   1. On mousedown, measure each child's pixel size via getBoundingClientRect.
//   2. Redistribute the boundary delta between the two adjacent children
//      only — siblings outside the drag stay fixed. Preserves total size.
//   3. Convert to proportional weights (divide by the sum); the store
//      clamps each weight to MIN_SPLIT_WEIGHT so a child never vanishes.
//   4. Apply via workspace.setSplitSizes, which persists through the
//      existing installAutoSave → saveWorkspace pipeline.
//
// The handle renders a 4px gutter between children (matching DockResizeHandle
// styling). No-op when the split has fewer than 2 live child DOM nodes —
// defensive against a ref race on hydrate.
// ---------------------------------------------------------------------------

function SplitResizeHandle({
  splitId,
  boundaryIndex,
  horizontal,
  childRefs,
  currentSizes,
}: {
  splitId: string
  boundaryIndex: number
  horizontal: boolean
  childRefs: React.MutableRefObject<(HTMLDivElement | null)[]>
  currentSizes: number[] | undefined
}): JSX.Element {
  const startPos = useRef(0)
  const startPixels = useRef<number[]>([])

  const onMouseDown = (e: React.MouseEvent): void => {
    e.preventDefault()
    startPos.current = horizontal ? e.clientX : e.clientY
    const refs = childRefs.current
    startPixels.current = refs.map((el) => {
      if (!el) return 0
      const rect = el.getBoundingClientRect()
      return horizontal ? rect.width : rect.height
    })
    // If we never wired all children (component re-renders can leave
    // stale slots), bail silently — dragging the handle in that state
    // does nothing rather than corrupt the sizes array.
    if (startPixels.current.some((px) => px <= 0)) return

    const onMouseMove = (ev: MouseEvent): void => {
      const cur = horizontal ? ev.clientX : ev.clientY
      const delta = cur - startPos.current
      const pixels = [...startPixels.current]
      // Transfer the delta from boundaryIndex+1 into boundaryIndex.
      // Positive delta (drag right / down) shrinks the right/bottom
      // child and grows the left/top child by the same amount.
      pixels[boundaryIndex] += delta
      pixels[boundaryIndex + 1] -= delta
      // Normalise to proportional weights so the result is resolution-
      // independent (window resize rescales all children together).
      const total = pixels.reduce((sum, p) => sum + p, 0)
      if (total <= 0) return
      const weights = pixels.map((p) => p / total)
      workspace.setSplitSizes(splitId, weights)
    }

    const cleanup = (): void => {
      document.removeEventListener('mousemove', onMouseMove)
      document.removeEventListener('mouseup', cleanup)
    }

    document.addEventListener('mousemove', onMouseMove)
    document.addEventListener('mouseup', cleanup)
  }

  // `currentSizes` is read so the handle re-mounts when the split
  // arity changes — a dangling drag that started before an adjacent
  // pane closed would otherwise write a mismatched array to the store,
  // which `setSplitSizes` rejects but still burns a no-op event.
  void currentSizes

  return (
    <div
      className="workspace-split-resize-handle"
      onMouseDown={onMouseDown}
      style={
        horizontal
          ? {
              flex: '0 0 2px',
              width: 2,
              cursor: 'col-resize',
              background: 'transparent',
              zIndex: 1,
            }
          : {
              flex: '0 0 2px',
              height: 2,
              width: '100%',
              cursor: 'row-resize',
              background: 'transparent',
              zIndex: 1,
            }
      }
    />
  )
}

function childKey(node: WorkspaceParent): string {
  return (node as { id?: string }).id ?? 'anon'
}

// ---------------------------------------------------------------------------
// <TabGroup> — TabStrip on top + all LeafHosts (inactive hidden via display).
// CRITICAL (plan line 134): render ALL leaves, hide inactive with display:none.
// ---------------------------------------------------------------------------

interface TabGroupProps {
  tabs: Tabs
  isMainDock: boolean
  sideDock?: 'left' | 'right'
  hideTabStrip?: boolean
}

function TabGroup({ tabs, isMainDock, sideDock, hideTabStrip }: TabGroupProps): JSX.Element {
  const activeLeaf = tabs.leaves[tabs.activeIndex] ?? null

  return (
    <div
      className="workspace-tab-group"
      style={{
        display: 'flex',
        flexDirection: 'column',
        flex: '1 1 auto',
        minWidth: 0,
        minHeight: 0,
        width: '100%',
        height: '100%',
        overflow: 'hidden',
      }}
    >
      {!hideTabStrip && <TabStrip tabs={tabs} isMainDock={isMainDock} sideDock={sideDock} />}
      <div
        className="workspace-tab-body"
        style={{
          flex: '1 1 auto',
          minHeight: 0,
          position: 'relative',
          display: 'flex',
        }}
      >
        {tabs.leaves.map((leaf) => (
          <LeafHost
            key={leaf.id}
            leaf={leaf}
            hidden={leaf !== activeLeaf}
          />
        ))}
      </div>
    </div>
  )
}

// ---------------------------------------------------------------------------
// <TabStrip> — horizontal list of tab buttons.
// ---------------------------------------------------------------------------

function TabStrip({
  tabs,
  isMainDock = false,
  sideDock,
}: {
  tabs: Tabs
  isMainDock?: boolean
  sideDock?: 'left' | 'right'
}): JSX.Element {
  const handleNewTab = (): void => {
    const leaf = workspace.createLeaf(tabs)
    tabs.leaves.push(leaf)
    tabs.activeIndex = tabs.leaves.length - 1
    // Seed the leaf with the empty viewType so its creator runs and
    // the action-links placeholder mounts. Without this `leaf.view`
    // stays null and LeafHost renders a blank container.
    void leaf.setViewState({ type: 'empty', active: true })
    workspace.setActiveLeaf(leaf)
    workspace.emit('layout-change')
  }

  // Trailing buttons (chevron, right-sidedock toggle) match WindowControls'
  // 40×36 footprint so the cluster of icons running across the top-right
  // — chevron, panel-toggle, min, max, close — reads as a single tidy
  // row instead of a ragged mix of 28/34/40-px buttons.
  const collapseButtonStyle: CSSProperties = {
    width: 40,
    height: 36,
    background: 'transparent',
    border: 'none',
    color: 'var(--text-muted, #888)',
    cursor: 'pointer',
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 0,
    fontSize: 14,
    lineHeight: 1,
    flex: '0 0 auto',
  }

  // Trailing controls (chevron, panel-toggle) must never shrink or be
  // clipped — at narrow window widths they would otherwise slide under
  // the absolutely-positioned WindowControls (z-index 100). They live
  // in a fixed-width sibling container so the tabs scroll independently.
  const reservesWindowControls =
    sideDock === 'right' || (isMainDock && workspace.rightSplit.collapsed)

  return (
    <div
      className="workspace-tab-strip"
      role="tablist"
      style={{
        display: 'flex',
        flexDirection: 'row',
        flex: '0 0 auto',
        background: 'var(--tab-container-background, var(--background-secondary-alt, #2d2d2d))',
        borderBottom: '1px solid var(--divider-color, var(--background-modifier-border, #333))',
        // 36px matches the SidebarToggleButton + activity-bar items'
        // top-row baseline so the center tab row, the right sidedock
        // tab row, and the left activity bar's first icon row all
        // sit on the same horizontal line. Use a fixed `height`
        // (not `min-height`) so the strip stays exactly 36px even
        // when a child like a 36-tall sidebar-tab would otherwise
        // push the box to 37 with the 1px border-bottom — that
        // would slip the next row (file-tree toolbar) 1px below the
        // main dock's view-header.
        height: 36,
        overflow: 'hidden',
        // Reserve horizontal space at the trailing edge for the absolute
        // WindowControls cluster (3 × 40px = 120px) when this tab strip is
        // the rightmost visible column — plus an 8px gap so the trailing
        // tab controls (chevron + sidedock toggle) don't butt directly up
        // against min/max/close. That's the right sidedock when expanded,
        // or the main dock when the right sidedock is collapsed.
        ...(reservesWindowControls ? { paddingRight: 128 } : {}),
      }}
    >
      <div
        className="workspace-tab-strip-scroll"
        style={{
          display: 'flex',
          flexDirection: 'row',
          flex: '1 1 auto',
          minWidth: 0,
          overflowX: 'auto',
          overflowY: 'hidden',
          scrollbarWidth: 'none',
        }}
      >
        {tabs.leaves.map((leaf, i) => {
          // Hide placeholder `empty` leaves from sidedock tab strips —
          // they have no icon to render, and the fallback (first letter
          // of viewType) would surface as a row of "E" buttons. Main
          // dock keeps the tab so the user can reassign the leaf.
          if (sideDock && leaf.view?.viewType === 'empty') return null
          const isActive = i === tabs.activeIndex
          return (
            <TabButton
              key={leaf.id}
              leaf={leaf}
              active={isActive}
              canClose={tabs.leaves.length > 1}
              onActivate={() => workspace.setTabActiveIndex(tabs.id, i)}
              onClose={() => {
                void workspace.detachLeaf(leaf)
              }}
              sideDock={sideDock}
            />
          )
        })}
        {isMainDock && (
          <button
            type="button"
            aria-label="New tab"
            title="New tab"
            onClick={handleNewTab}
            className="workspace-tab-new"
            style={{
              background: 'transparent',
              border: 'none',
              color: 'var(--text-muted, #888)',
              cursor: 'pointer',
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              padding: '0 8px',
              fontSize: 12,
              lineHeight: 1,
              flex: '0 0 auto',
            }}
          >
            <Icon name="plus" size={14} />
          </button>
        )}
        {/* Flex spacer — drag region. Fills the unused horizontal space
            inside the scrollable container so the empty area beside the
            tabs remains a window-drag target. Tauri's drag region only
            applies to elements explicitly marked, so individual tabs
            and buttons keep their click semantics. */}
        <div
          style={{ flex: '1 1 auto', alignSelf: 'stretch', minWidth: 0 }}
          data-tauri-drag-region
        />
      </div>
      {/* Trailing controls — pinned to the right with flex-shrink: 0 so
          they never collide with the WindowControls or get clipped at
          narrow window widths. */}
      {isMainDock && (
        <div
          style={{
            display: 'flex',
            flexDirection: 'row',
            alignItems: 'stretch',
            flex: '0 0 auto',
          }}
        >
          <TabListDropdown tabs={tabs} />
          {/* Right-sidedock collapse / expand — mirrors the activity-bar
              left-panel toggle. Uses the PanelRight icon so the affordance
              matches the left toggle. Toggles collapsed state, so it also
              re-expands the dock once hidden. */}
          <button
            type="button"
            className="workspace-tab-strip-right-toggle"
            aria-label={
              workspace.rightSplit.collapsed
                ? 'Show right sidebar'
                : 'Hide right sidebar'
            }
            title={
              workspace.rightSplit.collapsed
                ? 'Show right sidebar'
                : 'Hide right sidebar'
            }
            onClick={() =>
              workspace.setSidedockCollapsed('right', !workspace.rightSplit.collapsed)
            }
            style={collapseButtonStyle}
          >
            <Icon name="panel" size={18} />
          </button>
        </div>
      )}
      {/* Left sidebar collapse lives in the activity bar (single source
          of truth); no duplicate affordance in the tab strip. */}
    </div>
  )
}

// ---------------------------------------------------------------------------
// <TabListDropdown> — Obsidian-style `v` chevron next to the + new-tab
// button. Opens a menu listing the current tab titles (click to
// activate) plus a "Close all" action. Positioned fixed so it floats
// over the editor content.
// ---------------------------------------------------------------------------

function TabListDropdown({ tabs }: { tabs: Tabs }): JSX.Element {
  const [open, setOpen] = useState(false)
  const anchorRef = useRef<HTMLButtonElement | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)

  // Close on outside click.
  useEffect(() => {
    if (!open) return
    const onDocClick = (e: MouseEvent) => {
      const target = e.target as Node
      if (menuRef.current?.contains(target)) return
      if (anchorRef.current?.contains(target)) return
      setOpen(false)
    }
    const onEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onDocClick)
    document.addEventListener('keydown', onEscape)
    return () => {
      document.removeEventListener('mousedown', onDocClick)
      document.removeEventListener('keydown', onEscape)
    }
  }, [open])

  const rect = anchorRef.current?.getBoundingClientRect()

  const closeAll = () => {
    const snapshot = [...tabs.leaves]
    for (const leaf of snapshot) {
      void workspace.detachLeaf(leaf)
    }
    setOpen(false)
  }

  return (
    <>
      <button
        ref={anchorRef}
        type="button"
        aria-label="Tab list"
        title="Tab list"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        style={{
          width: 40,
          height: 36,
          background: 'transparent',
          border: 'none',
          color: 'var(--text-muted, #888)',
          cursor: 'pointer',
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          padding: 0,
          fontSize: 12,
          lineHeight: 1,
          flex: '0 0 auto',
        }}
      >
        <Icon name="chevDown" size={14} />
      </button>
      {open && rect && (
        <div
          ref={menuRef}
          role="menu"
          style={{
            position: 'fixed',
            top: rect.bottom + 4,
            left: Math.max(4, rect.right - 240),
            zIndex: zIndex.dropdown,
            minWidth: 220,
            background: 'var(--background-primary, #1e1e1e)',
            border: '1px solid var(--divider-color, var(--background-modifier-border, #333))',
            borderRadius: 6,
            boxShadow: '0 6px 20px rgba(0,0,0,0.35)',
            padding: 4,
            fontSize: 12,
          }}
        >
          {tabs.leaves.map((leaf, i) => {
            const isActive = i === tabs.activeIndex
            const label =
              leaf.view?.getDisplayText?.() ?? leaf.view?.viewType ?? 'Empty'
            // BL-029 — disable popout on placeholder `empty` leaves
            // since there's nothing to host in a separate window yet.
            const canPopout = leaf.view?.viewType !== 'empty'
            return (
              <div
                key={leaf.id}
                role="group"
                style={{
                  display: 'flex',
                  alignItems: 'stretch',
                  width: '100%',
                  borderRadius: 4,
                }}
                onMouseEnter={(e) => {
                  ;(e.currentTarget as HTMLDivElement).style.background =
                    'var(--background-modifier-hover, #2a2a2a)'
                }}
                onMouseLeave={(e) => {
                  ;(e.currentTarget as HTMLDivElement).style.background = 'transparent'
                }}
              >
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    workspace.setTabActiveIndex(tabs.id, i)
                    setOpen(false)
                  }}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    flex: '1 1 auto',
                    padding: '6px 8px',
                    background: 'transparent',
                    border: 'none',
                    color: 'var(--text-normal, #ccc)',
                    cursor: 'pointer',
                    textAlign: 'left',
                    gap: 8,
                    minWidth: 0,
                  }}
                >
                  <span style={{ width: 12, display: 'inline-flex' }}>
                    {isActive ? <Icon name="check" size={12} /> : null}
                  </span>
                  <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {label}
                  </span>
                </button>
                {/* BL-029 — pop out this tab into a separate OS
                    window. Click is gated behind stopPropagation so
                    the parent row's activate-tab handler doesn't
                    fire alongside. The Tauri bridge handles the
                    layout mutation + window creation; failure rolls
                    the layout edit back. */}
                <button
                  type="button"
                  aria-label={`Pop out ${label}`}
                  title="Pop out tab"
                  disabled={!canPopout}
                  onClick={(e) => {
                    e.stopPropagation()
                    if (!canPopout) return
                    setOpen(false)
                    void popoutLeafBridge(leaf.id, { title: label })
                  }}
                  style={{
                    display: 'inline-flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    width: 28,
                    flex: '0 0 auto',
                    background: 'transparent',
                    border: 'none',
                    color: 'var(--text-muted, #888)',
                    cursor: canPopout ? 'pointer' : 'default',
                    opacity: canPopout ? 1 : 0.3,
                    padding: 0,
                  }}
                >
                  <Icon name="external" size={12} />
                </button>
              </div>
            )
          })}
          <div
            style={{
              height: 1,
              background: 'var(--divider-color, var(--background-modifier-border, #333))',
              margin: '4px 0',
            }}
          />
          <button
            type="button"
            role="menuitem"
            onClick={closeAll}
            disabled={tabs.leaves.length <= 1}
            style={{
              display: 'flex',
              alignItems: 'center',
              width: '100%',
              padding: '6px 8px',
              background: 'transparent',
              border: 'none',
              color: 'var(--text-normal, #ccc)',
              cursor: tabs.leaves.length <= 1 ? 'default' : 'pointer',
              opacity: tabs.leaves.length <= 1 ? 0.4 : 1,
              textAlign: 'left',
              borderRadius: 4,
              gap: 8,
            }}
            onMouseEnter={(e) => {
              if (tabs.leaves.length <= 1) return
              ;(e.currentTarget as HTMLButtonElement).style.background =
                'var(--background-modifier-hover, #2a2a2a)'
            }}
            onMouseLeave={(e) => {
              ;(e.currentTarget as HTMLButtonElement).style.background = 'transparent'
            }}
          >
            <span style={{ width: 12 }} />
            <span>Close all</span>
          </button>
        </div>
      )}
    </>
  )
}

interface TabButtonProps {
  leaf: Leaf
  active: boolean
  canClose: boolean
  onActivate: () => void
  onClose: () => void
  /** When set, this tab lives in a sidedock and should render as an
   *  icon-only button with no ×-close (Obsidian sidebar-tab pattern).
   *  Left-undefined for main-dock tabs, which keep label + close. */
  sideDock?: 'left' | 'right'
}

/** Fallback viewType → icon mapping for sidebar tabs. Used when a
 *  view doesn't implement `getIcon()`. Keep in sync with the plugin
 *  viewType keys. */
const SIDEBAR_VIEW_ICON_MAP: Record<string, string> = {
  'file-explorer': 'folder',
  search: 'search',
  bookmarks: 'star',
  outline: 'list',
  backlinks: 'linkIn',
  backlink: 'linkIn', // plugin registers under singular viewType
  graph: 'graph',
  tags: 'tag',
  'all-properties': 'archive', // Obsidian-parity icon
  'outgoing-links': 'linkOut',
  'file-properties': 'info',
}

function iconForSidebarLeaf(leaf: Leaf): string | null {
  const fromView = leaf.view?.getIcon?.()
  if (fromView) return fromView
  const viewType = leaf.view?.viewType
  if (viewType && SIDEBAR_VIEW_ICON_MAP[viewType]) {
    return SIDEBAR_VIEW_ICON_MAP[viewType]
  }
  return null
}

function TabButton({
  leaf,
  active,
  canClose,
  onActivate,
  onClose,
  sideDock,
}: TabButtonProps): JSX.Element {
  // Views may override `getDisplayText()` to show a per-instance label
  // (e.g. markdown shows the filename). Fall back to `viewType`, or
  // "Empty" when the view is null.
  const label =
    leaf.view?.getDisplayText?.() ?? leaf.view?.viewType ?? 'Empty'
  const closable = canClose && !leaf.pinned && !sideDock

  // Sidebar tabs: icon-only, square, no text, no close ×.
  if (sideDock) {
    const iconName = iconForSidebarLeaf(leaf)
    return (
      <button
        type="button"
        role="tab"
        aria-selected={active}
        aria-label={label}
        title={label}
        onClick={onActivate}
        className={`workspace-tab sidebar-tab${active ? ' is-active' : ''}`}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          width: 36,
          height: 36,
          padding: 0,
          cursor: 'pointer',
          background: active
            ? 'var(--background-primary, #1e1e1e)'
            : 'transparent',
          color: active
            ? 'var(--text-normal, #ccc)'
            : 'var(--text-muted, #888)',
          border: 'none',
        }}
      >
        {iconName ? (
          // Match the activity-bar icon size so a sidebar tab and the
          // adjacent ribbon button line up at the same visual scale.
          <Icon name={iconName as never} size={18} />
        ) : (
          // Final fallback: a neutral dot for views with no icon
          // mapping. Earlier this rendered the first letter of the
          // viewType uppercase, which surfaced as bare "E"/"X"/etc.
          // letters in the chrome — visually noisy and easy to mistake
          // for broken UI.
          <span
            aria-hidden
            style={{
              width: 6,
              height: 6,
              borderRadius: '50%',
              background: 'currentColor',
              opacity: 0.5,
            }}
          />
        )}
      </button>
    )
  }

  return (
    <div
      role="tab"
      aria-selected={active}
      onClick={onActivate}
      className={`workspace-tab${active ? ' is-active' : ''}`}
      title={label}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 4,
        padding: '2px 6px',
        cursor: 'pointer',
        background: active
          ? 'var(--background-primary, #1e1e1e)'
          : 'transparent',
        color: active
          ? 'var(--text-normal, #ccc)'
          : 'var(--text-muted, #888)',
        borderRight: '1px solid var(--divider-color, var(--background-modifier-border, #333))',
        fontSize: 11,
        whiteSpace: 'nowrap',
        // Each tab prefers ~180px but can shrink to ~50px when the
        // strip gets crowded. Won't grow past the basis even when
        // there are only 1-2 tabs (so a single tab doesn't stretch
        // across the whole strip).
        flex: '0 1 180px',
        minWidth: 50,
        maxWidth: 180,
      }}
    >
      {leaf.pinned && (
        <span aria-label="pinned" title="Pinned" style={{ fontSize: 10 }}>
          ●
        </span>
      )}
      <span
        style={{
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          minWidth: 0,
          flex: '1 1 auto',
        }}
      >
        {label}
      </span>
      {closable && (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation()
            onClose()
          }}
          title="Close tab"
          style={{
            background: 'transparent',
            border: 'none',
            color: 'inherit',
            cursor: 'pointer',
            padding: '0 2px',
            fontSize: 12,
            lineHeight: 1,
          }}
        >
          ×
        </button>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------
// <LeafHost> — the one place a View's DOM lives.
//
// The wrapper div has NO React children, so React has nothing to diff
// inside it. Only the `style` attribute changes between renders when
// `hidden` toggles — the imperative view DOM is untouched.
//
// memo(LeafHost, ...) additionally freezes re-renders when neither leaf
// nor hidden changed, preventing stray parent re-renders from reaching
// this subtree.
// ---------------------------------------------------------------------------

interface LeafHostProps {
  leaf: Leaf
  hidden: boolean
}

const LeafHostInner = ({ leaf, hidden }: LeafHostProps): JSX.Element => {
  const ref = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    void leaf.attachContainer(el)
    return () => {
      void leaf.attachContainer(null)
    }
  }, [leaf])

  return (
    <div
      ref={ref}
      className="workspace-leaf-host"
      data-leaf-id={leaf.id}
      style={{
        display: hidden ? 'none' : 'flex',
        flex: '1 1 auto',
        width: '100%',
        height: '100%',
        minWidth: 0,
        minHeight: 0,
        position: 'absolute',
        inset: 0,
      }}
    />
  )
}

export const LeafHost = memo(LeafHostInner, (prev, next) => {
  // Re-render only when the leaf identity or hidden flag changes. The
  // view's DOM is owned imperatively — nothing inside this div ever
  // changes from React's perspective.
  return prev.leaf === next.leaf && prev.hidden === next.hidden
})

// ---------------------------------------------------------------------------
// Named exports for the component set (plan requirement).
// ---------------------------------------------------------------------------

export { RenderNode, SidedockFrame, TabGroup, TabStrip }
