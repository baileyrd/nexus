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
import { WindowControls, IS_MACOS } from '../shell/WindowControls'
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
import { ContextMenu, type ContextMenuItem } from '../shell/ContextMenu.tsx'
import { getRegistry } from '../host/shellRegistry'
import { disableBuiltinPlugin } from '../host/pluginActivation'
import { ALL_PLUGINS } from '../plugins/catalog'
import { contextKeyService } from '../host/ContextKeyService'
import { clientLogger } from '../host/clientLogger'
import { useEditorStore, isDirty } from '../plugins/nexus/editor/editorStore'

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

  // SH-015: Update document.title on active-leaf-change so macOS shows the
  // current file in the proxy icon / window title bar (also benefits
  // Windows / Linux task-bar previews).
  useEffect(() => {
    const off = workspace.on('active-leaf-change', (payload: unknown) => {
      const leaf = (payload as { leaf?: { getDisplayText?: () => string } } | undefined)?.leaf
      const text = leaf?.getDisplayText?.()
      if (typeof text === 'string' && text.length > 0) {
        document.title = `${text} — Nexus`
      }
    })
    return off
  }, [])

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
      {/* SH-015: Floating window-controls anchor. macOS positions controls
          at top-left (traffic-light convention); Windows/Linux at top-right. */}
      <div
        style={{
          position: 'absolute',
          top: 0,
          ...(IS_MACOS ? { left: 0 } : { right: 0 }),
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
// SH-004: Default width for the collapsed-ribbon placeholder. The actual visual
// width is overridden by the CSS token --chrome-icon-size from the density block.
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
  width: 'var(--chrome-icon-size)',
  flex: '0 0 var(--chrome-icon-size)',
  display: 'flex',
  alignItems: 'flex-start',
  justifyContent: 'center',
  background: 'var(--background-secondary)',
  borderRight: '1px solid var(--divider-color)',
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
        background: 'var(--background-secondary)',
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
        background: 'var(--background-secondary)',
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
  // Tab drag-reorder state. `dragSrcIndex` is a ref (no re-render needed
  // to track the source); `dragOverIndex` is state so the drop indicator
  // hairline appears/disappears without a full strip remount.
  const dragSrcIndex = useRef<number | null>(null)
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null)

  // Tab right-click menu. Anchored at the mouse position (constructed as
  // a zero-size DOMRect so `ContextMenu`'s positioning logic places it
  // immediately below the cursor). One menu per strip — opening on a
  // different tab swaps the anchor leaf in place.
  const [tabMenu, setTabMenu] = useState<{
    leaf: Leaf
    anchorRect: DOMRect
  } | null>(null)

  const handleTabContextMenu = (leaf: Leaf, e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    const anchor = new DOMRect(e.clientX, e.clientY, 0, 0)
    setTabMenu({ leaf, anchorRect: anchor })
  }

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
  // footprint so the icon cluster across the top edge reads as a single
  // tidy row. SH-004: height tracks --chrome-row-height.
  const collapseButtonStyle: CSSProperties = {
    width: 'var(--chrome-row-height)',
    height: 'var(--chrome-row-height)',
    background: 'transparent',
    border: 'none',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 0,
    fontSize: 14,
    lineHeight: 1,
    flex: '0 0 auto',
  }

  // SH-015: Reserve edge space so tabs never slide under the absolute
  // WindowControls cluster. On Windows/Linux the cluster is top-right
  // (3×40=120px + 8px gap → paddingRight:128); on macOS it is top-left
  // (3×12px circles + gaps + 8px padding → ~60px, use paddingLeft:68
  // so the first tab stays clear of the traffic lights).
  const reservesWindowControls = IS_MACOS
    ? sideDock === 'left' || (isMainDock && workspace.leftSplit.collapsed)
    : sideDock === 'right' || (isMainDock && workspace.rightSplit.collapsed)
  const edgePadding = IS_MACOS
    ? (reservesWindowControls ? { paddingLeft: 68 } : {})
    : (reservesWindowControls ? { paddingRight: 128 } : {})

  return (
    <div
      className="workspace-tab-strip"
      role="tablist"
      style={{
        display: 'flex',
        flexDirection: 'row',
        flex: '0 0 auto',
        background: 'var(--tab-container-background)',
        borderBottom: '1px solid var(--divider-color)',
        // Height tracks --chrome-row-height (SH-004) so the tab strip
        // scales with density. Use a fixed `height` (not `min-height`)
        // so the strip stays exactly one row tall — if a child would
        // otherwise push the box 1px over, that slips the file-tree
        // toolbar below the main dock's view-header.
        height: 'var(--chrome-row-height)',
        overflow: 'hidden',
        ...edgePadding,
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
              onTabContextMenu={(e) => handleTabContextMenu(leaf, e)}
              sideDock={sideDock}
              // Drag-reorder — main-dock only (sideDock tabs use icon-only
              // buttons that live in a fixed rail; reordering them is not
              // currently supported and their button element has no space
              // for a visible drag affordance).
              {...(!sideDock && {
                isDragOver: dragOverIndex === i,
                onTabDragStart: () => { dragSrcIndex.current = i },
                onTabDragOver: (e) => {
                  e.preventDefault()
                  e.dataTransfer.dropEffect = 'move'
                  if (dragSrcIndex.current !== null && dragSrcIndex.current !== i) {
                    setDragOverIndex(i)
                  }
                },
                onTabDragLeave: () => setDragOverIndex(null),
                onTabDrop: (e) => {
                  e.preventDefault()
                  if (dragSrcIndex.current !== null && dragSrcIndex.current !== i) {
                    workspace.reorderLeaves(tabs.id, dragSrcIndex.current, i)
                  }
                  dragSrcIndex.current = null
                  setDragOverIndex(null)
                },
                onTabDragEnd: () => {
                  dragSrcIndex.current = null
                  setDragOverIndex(null)
                },
              })}
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
              color: 'var(--text-muted)',
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
      <ContextMenu
        open={tabMenu !== null}
        anchorRect={tabMenu?.anchorRect ?? null}
        items={tabMenu ? buildTabMenuItems(tabMenu.leaf) : []}
        onClose={() => setTabMenu(null)}
        align="start"
      />
    </div>
  )
}

/**
 * Build the right-click menu for a tab. The menu always offers Close.
 * If the leaf's viewType is owned by a known plugin (i.e. registered
 * via `api.viewRegistry.register`), it also offers the user a way to
 * jump to that plugin's Settings entry and to disable it outright —
 * disabling now removes the leaves live thanks to the
 * viewType-ownership sweep in `PluginRegistry.unregisterAll`.
 */
function buildTabMenuItems(leaf: Leaf): ContextMenuItem[] {
  const items: ContextMenuItem[] = []
  const label =
    leaf.view?.getDisplayText?.() ?? leaf.view?.viewType ?? 'Tab'

  items.push({
    kind: 'item',
    label: 'Close',
    iconName: 'x',
    onSelect: () => {
      void workspace.detachLeaf(leaf)
    },
    tooltip: `Close ${label}`,
  })

  const viewType = leaf.view?.viewType
  const reg = getRegistry()
  const ownerId = viewType && reg ? reg.ownerOfViewType(viewType) : null
  if (ownerId) {
    const entry = ALL_PLUGINS.find((p) => p.id === ownerId)
    const pluginName = entry?.name ?? ownerId

    items.push({ kind: 'separator' })
    items.push({ kind: 'header', label: `Plugin: ${pluginName}` })
    items.push({
      kind: 'item',
      label: 'Plugin settings…',
      iconName: 'settings',
      onSelect: () => {
        contextKeyService.set('settingsPanelVisible', true)
        contextKeyService.set('settingsActiveTab', 'plugins')
      },
    })

    // Only non-core plugins are disable-able mid-session — core
    // services would leave the shell in a half-broken state. The
    // catalog tracks which is which; gate the menu row off that flag
    // rather than letting the user trigger a guaranteed-failure call.
    const disablable = entry ? entry.core === false : false
    items.push({
      kind: 'item',
      label: 'Disable plugin',
      iconName: 'x',
      disabled: !disablable,
      tooltip: disablable
        ? `Disable ${pluginName} — closes all of its panels`
        : 'This plugin is a required built-in and cannot be disabled',
      onSelect: async () => {
        const result = await disableBuiltinPlugin(ownerId)
        if (!result.ok) {
          clientLogger.warn(
            `[workspace] disablePlugin('${ownerId}') failed: ${result.error}`,
          )
        }
      },
    })
  }

  return items
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
          width: 'var(--chrome-row-height)',
          height: 'var(--chrome-row-height)',
          background: 'transparent',
          border: 'none',
          color: 'var(--text-muted)',
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
            background: 'var(--background-primary)',
            border: '1px solid var(--divider-color)',
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
                    'var(--background-modifier-hover)'
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
                    color: 'var(--text-normal)',
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
                    color: 'var(--text-muted)',
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
              background: 'var(--divider-color)',
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
              color: 'var(--text-normal)',
              cursor: tabs.leaves.length <= 1 ? 'default' : 'pointer',
              opacity: tabs.leaves.length <= 1 ? 0.4 : 1,
              textAlign: 'left',
              borderRadius: 4,
              gap: 8,
            }}
            onMouseEnter={(e) => {
              if (tabs.leaves.length <= 1) return
              ;(e.currentTarget as HTMLButtonElement).style.background =
                'var(--background-modifier-hover)'
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
  /** Right-click handler. Both main-dock and sidedock variants support
   *  this — sidedock tabs would otherwise have no removal affordance
   *  at all, since their icon-only buttons have no room for a × and
   *  the surrounding chrome doesn't expose one either. */
  onTabContextMenu: (e: React.MouseEvent) => void
  /** When set, this tab lives in a sidedock and should render as an
   *  icon-only button with no ×-close (Obsidian sidebar-tab pattern).
   *  Left-undefined for main-dock tabs, which keep label + close. */
  sideDock?: 'left' | 'right'
  /** Drag-reorder props — only passed for main-dock tabs. */
  onTabDragStart?: () => void
  onTabDragOver?: (e: React.DragEvent) => void
  onTabDragLeave?: () => void
  onTabDrop?: (e: React.DragEvent) => void
  onTabDragEnd?: () => void
  isDragOver?: boolean
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

/**
 * Read a leaf's dirty state from the editor store. Returns `true` when
 * the leaf hosts a markdown view whose backing tab has unsaved edits,
 * `false` otherwise (non-markdown views, leaves without a relpath,
 * tabs the editor plugin doesn't know about). Subscribes via a Zustand
 * selector so the TabButton re-renders the dot when the tab flips
 * dirty / clean.
 */
function useLeafDirty(leaf: Leaf): boolean {
  const relpath = (() => {
    if (leaf.view?.viewType !== 'markdown') return undefined
    const st = leaf.view.getState() as { relpath?: unknown } | undefined
    return typeof st?.relpath === 'string' ? st.relpath : undefined
  })()
  // Deeper diagnostic for the markdown leaf — also subscribe to the
  // raw content+revision values so the effect re-fires when *any* of
  // them changes, even if the resulting `dirty` boolean doesn't flip.
  // Lets us see whether setContent and apply_transaction are reaching
  // the store at all.
  const probe = useEditorStore((s) => {
    if (!relpath) return null
    const tab = s.tabs.find((t) => t.relpath === relpath)
    if (!tab) return { tabFound: false } as const
    return {
      tabFound: true,
      contentLen: tab.content.length,
      savedContentLen: tab.savedContent.length,
      sessionRev: s.sessionRevision.get(relpath) ?? null,
      savedRev: s.savedRevision.get(relpath) ?? null,
      dirty: isDirty(tab, s),
    }
  })
  useEffect(() => {
    if (leaf.view?.viewType !== 'markdown') return
    clientLogger.info(
      `[useLeafDirty:md] leaf.id=${leaf.id} relpath=${String(relpath)} probe=${JSON.stringify(probe)}`,
    )
  }, [leaf.id, leaf.view, relpath, probe])
  if (!probe || probe.tabFound !== true) return false
  return probe.dirty === true
}

function TabButton({
  leaf,
  active,
  canClose,
  onActivate,
  onClose,
  onTabContextMenu,
  sideDock,
  onTabDragStart,
  onTabDragOver,
  onTabDragLeave,
  onTabDrop,
  onTabDragEnd,
  isDragOver,
}: TabButtonProps): JSX.Element {
  const dirty = useLeafDirty(leaf)
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
        onContextMenu={onTabContextMenu}
        className={`workspace-tab sidebar-tab${active ? ' is-active' : ''}`}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          width: 'var(--chrome-row-height)',
          height: 'var(--chrome-row-height)',
          padding: 0,
          cursor: 'pointer',
          background: active
            ? 'var(--background-primary)'
            : 'transparent',
          color: active
            ? 'var(--text-normal)'
            : 'var(--text-muted)',
          border: 'none',
        }}
      >
        {iconName ? (
          // Match the activity-bar icon size so a sidebar tab and the
          // adjacent ribbon button line up at the same visual scale.
          <Icon name={iconName as never} size={18} />
        ) : (
          // SH-008: 2-letter short-name from the viewType so AT users and
          // sighted users without icon mapping can identify the panel.
          // Derived from the raw viewType (e.g. "my-view" → "MY"), not
          // from the label, which can be a file path.
          <span
            aria-hidden
            style={{
              fontSize: 10,
              fontWeight: 600,
              letterSpacing: '0.02em',
              lineHeight: 1,
              opacity: 0.75,
              textTransform: 'uppercase',
              fontFamily: 'var(--font-interface)',
            }}
          >
            {(leaf.view?.viewType ?? label).replace(/[^a-zA-Z]/g, '').slice(0, 2) || '??'}
          </span>
        )}
      </button>
    )
  }

  return (
    <div
      role="tab"
      aria-selected={active}
      draggable={!!onTabDragStart}
      onClick={onActivate}
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = 'move'
        // setData is required by WebKit — without it the drop event
        // never fires, even if dragover calls preventDefault().
        e.dataTransfer.setData('text/plain', '')
        onTabDragStart?.()
      }}
      onDragOver={onTabDragOver}
      onDragLeave={(e) => {
        // dragleave fires when the cursor enters a child element
        // (label span, close button). relatedTarget is that child;
        // if it's still inside this tab we haven't really left.
        const related = e.relatedTarget as Node | null
        if (related && (e.currentTarget as HTMLElement).contains(related)) return
        onTabDragLeave?.()
      }}
      onDrop={onTabDrop}
      onDragEnd={onTabDragEnd}
      onContextMenu={onTabContextMenu}
      className={`workspace-tab${active ? ' is-active' : ''}`}
      title={label}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 4,
        padding: '2px 6px',
        cursor: 'pointer',
        background: active
          ? 'var(--background-primary)'
          : 'transparent',
        color: active
          ? 'var(--text-normal)'
          : 'var(--text-muted)',
        borderRight: '1px solid var(--divider-color)',
        // Drop indicator: 2px accent hairline on the left edge of the
        // target tab so the user sees exactly where the tab will land.
        boxShadow: isDragOver ? 'inset 2px 0 0 var(--interactive-accent)' : undefined,
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
      {dirty && (
        <span
          aria-label="Unsaved changes"
          title="Unsaved changes"
          style={{
            // Small filled circle in the accent colour — the
            // conventional "modified document" affordance.
            width: 8,
            height: 8,
            borderRadius: 999,
            background: 'var(--interactive-accent, currentColor)',
            flex: '0 0 auto',
            marginLeft: 2,
          }}
        />
      )}
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
