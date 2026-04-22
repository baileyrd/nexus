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
import {
  memo,
  useEffect,
  useReducer,
  useRef,
  type CSSProperties,
} from 'react'
import { Icon } from '../icons/index.tsx'
import { WindowControls } from '../shell/WindowControls'
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
          <RenderNode node={rootSplit} isMainDock />
        </div>
        <SidedockFrame side="right" dock={rightSplit} />
      </div>
      <SidedockFrame side="bottom" dock={bottomSplit} />
      {/* Floating window-controls anchor. Absolutely positioned at the
          window's top-right corner so it sits over whichever view /
          panel happens to render there, without introducing a new
          title-bar row that would stack beneath the native chrome. */}
      <div
        style={{
          position: 'absolute',
          top: 0,
          right: 0,
          zIndex: 100,
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
  background: 'var(--background-secondary, var(--bg-raised, #252526))',
  borderRight: '1px solid var(--divider-color, var(--line, #333))',
}

function SidedockFrame({ side, dock }: SidedockFrameProps): JSX.Element {
  if (side === 'bottom') return <BottomSidedockFrame dock={dock} />

  if (dock.collapsed) {
    // Right sidedock: fully hidden when collapsed. The re-expand
    // affordance lives in the main dock's tab strip (PanelRight icon
    // in the trailing edge of TabStrip), so no narrow bar is needed.
    if (side === 'right') return <></>

    // Left sidedock: render the 24-px bar with an expand chevron so
    // the user has a way to re-open it if they haven't bound the
    // activity-bar toggle or a keyboard shortcut.
    return (
      <div
        className={`workspace-sidedock mod-${side} is-collapsed`}
        style={{
          ...COLLAPSED_BAR_STYLE,
          borderRight: COLLAPSED_BAR_STYLE.borderRight,
          borderLeft: 'none',
        }}
      >
        <button
          type="button"
          title="Expand left sidebar"
          onClick={() => workspace.setSidedockCollapsed('left', false)}
          style={COLLAPSE_BUTTON_STYLE}
        >
          ›
        </button>
      </div>
    )
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
        background: 'var(--background-secondary, var(--bg-raised, #252526))',
        overflow: 'hidden',
      }}
    >
      {/* Collapse chevron now lives inside the TabStrip (threaded via
          sideDock prop) so left/center/right columns all have a single
          36px header row that lines up with WindowControls. */}
      <div style={{ flex: '1 1 auto', minHeight: 0, display: 'flex' }}>
        <RenderNode node={dock} sideDock={side} />
      </div>
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
  if (dock.collapsed) {
    return (
      <div
        className="workspace-sidedock mod-bottom is-collapsed"
        style={{
          flex: `0 0 ${BOTTOM_COLLAPSED_HEIGHT}px`,
          height: BOTTOM_COLLAPSED_HEIGHT,
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '0 8px',
          background: 'var(--background-secondary, var(--bg-raised, #252526))',
          borderTop: '1px solid var(--divider-color, var(--line, #333))',
        }}
      >
        <span
          style={{
            fontSize: 12,
            color: 'var(--text-muted, var(--fg-muted, #888))',
          }}
        >
          Terminal
        </span>
        <button
          type="button"
          title="Expand bottom drawer"
          onClick={() => workspace.setSidedockCollapsed('bottom', false)}
          style={COLLAPSE_BUTTON_STYLE}
        >
          {/* up-chevron when collapsed — click to expand upward */}
          ▴
        </button>
      </div>
    )
  }

  return (
    <div
      className="workspace-sidedock mod-bottom"
      style={{
        flex: `0 0 ${dock.size}px`,
        height: dock.size,
        minHeight: DOCK_MIN_SIZE,
        display: 'flex',
        flexDirection: 'column',
        background: 'var(--background-secondary, var(--bg-raised, #252526))',
        overflow: 'hidden',
      }}
    >
      {/* Top-edge resize handle: dragging up grows the drawer. */}
      <DockResizeHandle side="bottom" initialSize={dock.size} />
      <div
        style={{
          display: 'flex',
          justifyContent: 'flex-end',
          padding: '2px 4px',
          borderBottom: '1px solid var(--divider-color, var(--line, #333))',
        }}
      >
        <button
          type="button"
          title="Collapse bottom drawer"
          onClick={() => workspace.setSidedockCollapsed('bottom', true)}
          style={COLLAPSE_BUTTON_STYLE}
        >
          {/* down-chevron when expanded — click to collapse downward */}
          ▾
        </button>
      </div>
      <div style={{ flex: '1 1 auto', minHeight: 0, display: 'flex' }}>
        <RenderNode node={dock} />
      </div>
    </div>
  )
}

const COLLAPSE_BUTTON_STYLE: CSSProperties = {
  background: 'transparent',
  border: 'none',
  color: 'var(--text-muted, var(--fg-muted, #888))',
  cursor: 'pointer',
  fontSize: 14,
  lineHeight: 1,
  padding: '2px 6px',
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
              flex: '0 0 4px',
              height: 4,
              width: '100%',
              cursor: 'row-resize',
              background: 'transparent',
              zIndex: 1,
            }
          : {
              flex: '0 0 4px',
              width: 4,
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
}

function RenderNode({ node, isMainDock = false, sideDock }: RenderNodeProps): JSX.Element | null {
  switch (node.kind) {
    case 'split':
      return <SplitNode node={node} isMainDock={isMainDock} sideDock={sideDock} />
    case 'tabs':
      return <TabGroup tabs={node} isMainDock={isMainDock} sideDock={sideDock} />
    case 'root':
      return <RenderNode node={(node as Root).child} isMainDock={isMainDock} sideDock={sideDock} />
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

function SplitNode({ node, isMainDock, sideDock }: { node: Split; isMainDock: boolean; sideDock?: 'left' | 'right' }): JSX.Element {
  const style: CSSProperties = {
    display: 'flex',
    flexDirection: node.direction === 'horizontal' ? 'row' : 'column',
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
          <div
            key={childKey(child)}
            style={{
              flex: `${childFlex} ${childFlex} 0`,
              minWidth: 0,
              minHeight: 0,
              display: 'flex',
            }}
          >
            <RenderNode node={child} isMainDock={isMainDock} sideDock={sideDock} />
          </div>
        )
      })}
    </div>
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
}

function TabGroup({ tabs, isMainDock, sideDock }: TabGroupProps): JSX.Element {
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
      <TabStrip tabs={tabs} isMainDock={isMainDock} sideDock={sideDock} />
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

  const collapseButtonStyle: CSSProperties = {
    background: 'transparent',
    border: 'none',
    color: 'var(--text-muted, var(--fg-muted, #888))',
    cursor: 'pointer',
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: '0 8px',
    fontSize: 14,
    lineHeight: 1,
    flex: '0 0 auto',
  }

  return (
    <div
      className="workspace-tab-strip"
      role="tablist"
      style={{
        display: 'flex',
        flexDirection: 'row',
        flex: '0 0 auto',
        background: 'var(--tab-container-background, var(--bg-soft, #2d2d2d))',
        borderBottom: '1px solid var(--divider-color, var(--line, #333))',
        minHeight: 36,
        overflow: 'hidden',
        // Reserve horizontal space at the trailing edge for the absolute
        // WindowControls cluster (3 × 40px = 120px) when this tab strip is
        // the rightmost visible column. That's the right sidedock when
        // expanded, or the main dock when the right sidedock is collapsed.
        ...(sideDock === 'right' ||
        (isMainDock && workspace.rightSplit.collapsed)
          ? { paddingRight: 120 }
          : {}),
      }}
    >
      {tabs.leaves.map((leaf, i) => {
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
            color: 'var(--text-muted, var(--fg-muted, #888))',
            cursor: 'pointer',
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            padding: '0 8px',
            fontSize: 12,
            lineHeight: 1,
          }}
        >
          <Icon name="plus" size={14} />
        </button>
      )}
      {/* Right-sidedock collapse / expand — lives at the far right of the
          main dock's tab strip (mirroring the activity-bar left-panel
          toggle). Uses the PanelRight icon so the affordance matches the
          left toggle style. Toggles collapsed state, so it also serves
          as the re-expand control once hidden. */}
      {isMainDock && (
        <button
          type="button"
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
          style={{ ...collapseButtonStyle, marginLeft: 'auto' }}
        >
          <Icon name="panel" size={14} />
        </button>
      )}
      {/* Left sidedock collapse chevron — trailing edge so the row reads
          [tabs ...] [icon] and toggles the left sidebar. */}
      {sideDock === 'left' && (
        <button
          type="button"
          aria-label="Hide left sidebar"
          title="Hide left sidebar"
          onClick={() => workspace.setSidedockCollapsed('left', true)}
          style={{ ...collapseButtonStyle, marginLeft: 'auto' }}
        >
          <Icon name="panelLeft" size={14} />
        </button>
      )}
    </div>
  )
}

interface TabButtonProps {
  leaf: Leaf
  active: boolean
  canClose: boolean
  onActivate: () => void
  onClose: () => void
}

function TabButton({
  leaf,
  active,
  canClose,
  onActivate,
  onClose,
}: TabButtonProps): JSX.Element {
  // Views may override `getDisplayText()` to show a per-instance label
  // (e.g. markdown shows the filename). Fall back to `viewType`, or
  // "Empty" when the view is null.
  const label =
    leaf.view?.getDisplayText?.() ?? leaf.view?.viewType ?? 'Empty'
  const closable = canClose && !leaf.pinned

  return (
    <div
      role="tab"
      aria-selected={active}
      onClick={onActivate}
      className={`workspace-tab${active ? ' is-active' : ''}`}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        padding: '4px 10px',
        cursor: 'pointer',
        background: active
          ? 'var(--background-primary, var(--bg, #1e1e1e))'
          : 'transparent',
        color: active
          ? 'var(--text-normal, var(--fg, #ccc))'
          : 'var(--text-muted, var(--fg-muted, #888))',
        borderRight: '1px solid var(--divider-color, var(--line, #333))',
        fontSize: 12,
        whiteSpace: 'nowrap',
      }}
    >
      {leaf.pinned && (
        <span aria-label="pinned" title="Pinned" style={{ fontSize: 10 }}>
          ●
        </span>
      )}
      <span>{label}</span>
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
