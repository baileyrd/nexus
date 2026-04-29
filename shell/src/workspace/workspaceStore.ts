// Workspace store — owns the live layout tree (root + left/right sidedocks +
// floating windows), the leaf registry, and the internal event bus.
//
// Follows the design in /home/baileyrd/projects/nexus/docs/leaf-migration-plan.md
// §Phase 3. Mirrors Obsidian's `Workspace` responsibilities (see
// /home/baileyrd/projects/obsidian_reverse/docs/10-editor-shell.md §4).
//
// Framework-agnostic: no React, no Tauri imports. The Zustand store backs a
// future reactive UI (Phase 4); external callers use the `workspace`
// singleton facade — mirroring the pattern in `shell/src/registry/SlotRegistry.ts`.

import { create } from 'zustand'
import type {
  FloatingWindow,
  Leaf,
  SerializedFloating,
  SerializedLeaf,
  SerializedNode,
  SerializedRoot,
  SerializedSplit,
  SerializedTabs,
  Sidedock,
  Split,
  Tabs,
  WorkspaceJSON,
  WorkspaceParent,
} from './types.ts'
import { LeafImpl } from './Leaf.ts'
import { useWorkspaceStore as useForgeStore } from '../plugins/nexus/workspace/workspaceStore.ts'

type Listener = (payload?: unknown) => void
type EmitFn = (event: string, payload?: unknown) => void

interface WorkspaceStoreState {
  rootSplit: Split
  leftSplit: Sidedock
  rightSplit: Sidedock
  bottomSplit: Sidedock
  floating: FloatingWindow[]
  activeLeafId: string | null
  leaves: Map<string, Leaf>
  listeners: Map<string, Set<Listener>>
}

/** Generate a new node id. */
const newId = (): string => crypto.randomUUID()

/** Build a fresh `Tabs` node with the given leaves (or empty). */
function makeTabs(leaves: Leaf[] = []): Tabs {
  return { kind: 'tabs', id: newId(), leaves, activeIndex: 0 }
}

/** Build a fresh `Split` with one empty Tabs child (no leaves yet). */
function makeEmptySplit(direction: 'horizontal' | 'vertical' = 'horizontal'): Split {
  return { kind: 'split', id: newId(), direction, children: [makeTabs()] }
}

/** Build a fresh `Sidedock` with one empty Tabs child. */
function makeEmptySidedock(
  side: 'left' | 'right' | 'bottom',
  size = 300,
): Sidedock {
  return {
    kind: 'split',
    id: newId(),
    // Left/right docks stack vertically inside the dock column; the
    // bottom drawer is a horizontal strip under everything else.
    direction: side === 'bottom' ? 'horizontal' : 'vertical',
    children: [makeTabs()],
    side,
    // Bottom drawer starts collapsed because an expanded terminal is
    // intrusive for first-run users. Sides retain the previous default.
    collapsed: side === 'bottom',
    size,
  }
}

/**
 * Walk a WorkspaceParent subtree and return the first `Tabs` descendant
 * encountered by depth-first search, or null if none exists.
 */
function findFirstTabs(node: WorkspaceParent): Tabs | null {
  if (node.kind === 'tabs') return node
  if (node.kind === 'split') {
    for (const child of node.children) {
      const found = findFirstTabs(child)
      if (found) return found
    }
    return null
  }
  // root / floating: recurse into child
  const withChild = node as { child?: WorkspaceParent }
  if (withChild.child) return findFirstTabs(withChild.child)
  return null
}

/**
 * Ascend the tree from a leaf, walking through parent Tabs and Splits, to
 * find the nearest containing Sidedock (if any). Because Nexus leaves hold a
 * direct `parent` back-pointer that is only one step (the Tabs), and Tabs
 * don't themselves back-point to their split, this helper walks the whole
 * tree looking for a Tabs that contains the leaf and returns its dock
 * ancestor.
 */
function findAncestorSidedock(
  leaf: Leaf,
  roots: WorkspaceParent[],
): Sidedock | null {
  for (const root of roots) {
    const found = findLeafAncestry(leaf, root, [])
    if (!found) continue
    // `found` is the chain [root, ..., parentTabs] leading to the leaf.
    for (let i = found.length - 1; i >= 0; i--) {
      const node = found[i]
      if (isSidedock(node)) return node
    }
    return null
  }
  return null
}

function isSidedock(node: WorkspaceParent | undefined): node is Sidedock {
  return !!node && node.kind === 'split' && 'side' in node
}

/**
 * Return the ancestry chain from `root` to the Tabs containing `leaf`, or
 * null if `leaf` is not in this subtree.
 */
function findLeafAncestry(
  leaf: Leaf,
  node: WorkspaceParent,
  chain: WorkspaceParent[],
): WorkspaceParent[] | null {
  const nextChain = [...chain, node]
  if (node.kind === 'tabs') {
    return node.leaves.some(l => l.id === leaf.id) ? nextChain : null
  }
  if (node.kind === 'split') {
    for (const child of node.children) {
      const r = findLeafAncestry(leaf, child, nextChain)
      if (r) return r
    }
    return null
  }
  const withChild = node as { child?: WorkspaceParent }
  if (withChild.child) return findLeafAncestry(leaf, withChild.child, nextChain)
  return null
}

// ---------------------------------------------------------------------------
// Zustand store
// ---------------------------------------------------------------------------

export const useWorkspaceStore = create<WorkspaceStoreState>(() => ({
  rootSplit: makeEmptySplit('horizontal'),
  leftSplit: makeEmptySidedock('left'),
  rightSplit: makeEmptySidedock('right'),
  bottomSplit: makeEmptySidedock('bottom', 240),
  floating: [],
  activeLeafId: null,
  leaves: new Map(),
  listeners: new Map(),
}))

// ---------------------------------------------------------------------------
// Internal helpers that mutate the store.
//
// We use `setState(s => ({ ... }))` for shallow state-field updates, but
// deliberately mutate the tree nodes in-place for leaf-level mutations —
// the nodes are shared with React via the singleton facade and this keeps
// referential stability for Phase 4's render layer. `emit('layout-change')`
// is the signal for subscribers that a structural change occurred.
// ---------------------------------------------------------------------------

function state(): WorkspaceStoreState {
  return useWorkspaceStore.getState()
}

/**
 * Internal emit. Route `active-leaf-change` through the bridge:
 *
 * Recursion-break strategy: `setActiveLeaf` updates `activeLeafId` *before*
 * calling `emit`. So when a leaf emits `active-leaf-change` from inside its
 * own `setViewState`, the bridge sees `payload.leaf.id !== activeLeafId` and
 * calls `setActiveLeaf(leaf)`. `setActiveLeaf` updates state then calls
 * `emit` again — but this time `activeLeafId === payload.leaf.id`, so the
 * bridge no-ops and the event is fanned out to subscribers exactly once.
 * Picked this approach over a separate "leaf-requested-active" event name
 * so that `Leaf.ts` does not need a Phase-3-specific branch.
 */
function emitInternal(event: string, payload?: unknown): void {
  if (event === 'active-leaf-change') {
    const p = payload as { leaf?: Leaf } | undefined
    const leaf = p?.leaf
    if (leaf && leaf.id !== state().activeLeafId) {
      // Bridge — update state, which re-enters `emit` with state now in sync.
      setActiveLeaf(leaf)
      return
    }
  }
  const set = state().listeners.get(event)
  if (!set) return
  for (const listener of set) {
    listener(payload)
  }
}

/** Bound emit — passed to every created leaf. */
const boundEmit: EmitFn = (event, payload) => emitInternal(event, payload)

function createLeaf(parent: WorkspaceParent): Leaf {
  const leaf = new LeafImpl(parent, boundEmit)
  const leaves = new Map(state().leaves)
  leaves.set(leaf.id, leaf)
  useWorkspaceStore.setState({ leaves })
  return leaf
}

/** Look up the sidedock for the given side. */
function dockForSide(side: 'left' | 'right' | 'bottom'): Sidedock {
  if (side === 'left') return state().leftSplit
  if (side === 'right') return state().rightSplit
  return state().bottomSplit
}

/** Return the first leaf in a sidedock, creating Tabs/Leaf if empty. */
function getSideLeaf(side: 'left' | 'right', reveal?: boolean): Leaf {
  const dock = dockForSide(side)
  let tabs = findFirstTabs(dock)
  if (!tabs) {
    tabs = makeTabs()
    dock.children.push(tabs)
  }
  let leaf: Leaf
  if (tabs.leaves.length === 0) {
    leaf = createLeaf(tabs)
    tabs.leaves.push(leaf)
    tabs.activeIndex = 0
    emitInternal('layout-change')
  } else {
    leaf = tabs.leaves[0]!
  }
  if (reveal) revealLeaf(leaf)
  return leaf
}

async function ensureLeafOfType(
  type: string,
  side: 'left' | 'right' | 'bottom' | 'main',
): Promise<Leaf> {
  // Existence only: if any leaf in the workspace already has this viewType,
  // return it unchanged. Never move, never reveal. (Plan resolved decision #2.)
  for (const leaf of state().leaves.values()) {
    if (leaf.view?.viewType === type) return leaf
  }
  // 'main' routes into the root split (center panel) rather than a sidedock,
  // so workbench-level views (workflow, mcp, skills, ai) don't clutter the
  // note-sidecar tab strip on the right.
  const parent = side === 'main' ? state().rootSplit : dockForSide(side)
  let tabs = findFirstTabs(parent)
  if (!tabs) {
    tabs = makeTabs()
    parent.children.push(tabs)
  }
  const leaf = createLeaf(tabs)
  tabs.leaves.push(leaf)
  // Caller-facing semantics: do NOT change activeIndex, collapsed, or emit
  // active-leaf-change. Only fire `layout-change` so renderers re-layout.
  await leaf.setViewState({ type })
  emitInternal('layout-change')
  return leaf
}

function revealLeaf(leaf: Leaf): void {
  // Find the ancestor chain to discover dock + parent Tabs in one pass.
  const roots: WorkspaceParent[] = [
    state().rootSplit,
    state().leftSplit,
    state().rightSplit,
    state().bottomSplit,
    ...state().floating,
  ]
  let chain: WorkspaceParent[] | null = null
  for (const root of roots) {
    chain = findLeafAncestry(leaf, root, [])
    if (chain) break
  }
  if (!chain) return

  // Expand the nearest sidedock if collapsed.
  for (const node of chain) {
    if (isSidedock(node) && node.collapsed) {
      node.collapsed = false
    }
  }

  // Parent Tabs is the last element of the chain; activate this leaf within it.
  const parentTabs = chain[chain.length - 1]
  if (parentTabs && parentTabs.kind === 'tabs') {
    const idx = parentTabs.leaves.findIndex(l => l.id === leaf.id)
    if (idx >= 0) parentTabs.activeIndex = idx
  }

  setActiveLeaf(leaf)
  emitInternal('layout-change')
}

function getLeavesOfType(type: string): Leaf[] {
  const result: Leaf[] = []
  for (const leaf of state().leaves.values()) {
    if (leaf.view?.viewType === type) result.push(leaf)
  }
  return result
}

function setActiveLeaf(leaf: Leaf): void {
  if (state().activeLeafId === leaf.id) {
    // Still emit — callers may want to re-assert active for focus side-effects.
    // But skip the state update to avoid unnecessary churn.
    emitInternal('active-leaf-change', { leaf })
    return
  }
  useWorkspaceStore.setState({ activeLeafId: leaf.id })
  emitInternal('active-leaf-change', { leaf })
}

/** Clamp + write `dock.size`; emit `layout-change`. Minimum size 150 (plan §Phase 4). */
function setSidedockSize(
  side: 'left' | 'right' | 'bottom',
  size: number,
): void {
  const dock = dockForSide(side)
  const clamped = Math.max(150, Math.floor(size))
  if (dock.size === clamped) return
  dock.size = clamped
  emitInternal('layout-change')
}

/**
 * OI-02 — drag-to-resize an internal `Split`. Finds the split by id,
 * validates arity (array length must match children count — a mismatch
 * from a stale drag after a split mutation is ignored), enforces a
 * minimum proportional size per child (treats each entry as a flex
 * weight and floors it so a child can never collapse to zero), and
 * emits `layout-change` which `installAutoSave` already persists via
 * `.forge/workspace.json`. No-op when the split isn't found or the
 * proposed sizes equal the current ones (avoids a redundant save).
 *
 * Proportional sizes (not pixels): `SplitNode` maps them to
 * `flex: <weight> <weight> 0`, so relative ratios are all that
 * matters. We still clamp each weight to `MIN_SPLIT_WEIGHT` to keep a
 * small-but-visible child lane when the user drags near the edge —
 * the rendered pixel floor is not enforced here because it depends
 * on viewport size, but drag code converts from pixel rects before
 * calling in so the clamped weight preserves the same proportional
 * visibility across window resizes.
 */
const MIN_SPLIT_WEIGHT = 0.1

function setSplitSizes(splitId: string, sizes: number[]): void {
  const roots: WorkspaceParent[] = [
    state().rootSplit,
    state().leftSplit,
    state().rightSplit,
    state().bottomSplit,
    ...state().floating,
  ]
  const split = findSplitById(splitId, roots)
  if (!split) return
  if (sizes.length !== split.children.length) return
  const clamped = sizes.map((s) => Math.max(MIN_SPLIT_WEIGHT, s))
  if (split.sizes && arraysEqual(split.sizes, clamped)) return
  split.sizes = clamped
  emitInternal('layout-change')
}

function arraysEqual(a: readonly number[], b: readonly number[]): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    if (Math.abs(a[i] - b[i]) > 1e-6) return false
  }
  return true
}

function findSplitById(id: string, roots: WorkspaceParent[]): Split | null {
  for (const root of roots) {
    const hit = findSplitRec(id, root)
    if (hit) return hit
  }
  return null
}

function findSplitRec(id: string, node: WorkspaceParent): Split | null {
  if (node.kind === 'split') {
    if (node.id === id) return node
    for (const child of node.children) {
      const hit = findSplitRec(id, child)
      if (hit) return hit
    }
  }
  if (node.kind === 'root') {
    return findSplitRec(id, (node as { child: WorkspaceParent }).child)
  }
  if (node.kind === 'floating') {
    return findSplitRec(id, (node as { child: WorkspaceParent }).child)
  }
  return null
}

/** Set `dock.collapsed`; emit `layout-change`. */
function setSidedockCollapsed(
  side: 'left' | 'right' | 'bottom',
  collapsed: boolean,
): void {
  const dock = dockForSide(side)
  if (dock.collapsed === collapsed) return
  dock.collapsed = collapsed
  emitInternal('layout-change')
}

/**
 * Walk the tree for a Tabs node by id; set its `activeIndex` and mark the
 * newly-selected leaf active. Emits `layout-change` + `active-leaf-change`.
 */
function setTabActiveIndex(tabsId: string, index: number): void {
  const roots: WorkspaceParent[] = [
    state().rootSplit,
    state().leftSplit,
    state().rightSplit,
    state().bottomSplit,
    ...state().floating,
  ]
  const found = findTabsById(tabsId, roots)
  if (!found) return
  if (index < 0 || index >= found.leaves.length) return
  if (found.activeIndex === index) return
  found.activeIndex = index
  emitInternal('layout-change')
  const selected = found.leaves[index]
  if (selected) setActiveLeaf(selected)
}

function findTabsById(id: string, roots: WorkspaceParent[]): Tabs | null {
  const visit = (n: WorkspaceParent): Tabs | null => {
    if (n.kind === 'tabs') return n.id === id ? n : null
    if (n.kind === 'split') {
      for (const c of n.children) {
        const r = visit(c)
        if (r) return r
      }
      return null
    }
    const withChild = n as { child?: WorkspaceParent }
    if (withChild.child) return visit(withChild.child)
    return null
  }
  for (const r of roots) {
    const t = visit(r)
    if (t) return t
  }
  return null
}

// ---------------------------------------------------------------------------
// BL-029 — popout / floating window mutations.
//
// Splitting the OS-side window control off from the layout mutation lets us:
//  1. Unit-test these helpers without a Tauri runtime.
//  2. Keep the workspaceStore pure (no @tauri-apps/api import).
// The Tauri-side bridge (`popoutWindowBridge.ts`) wraps the invoke calls
// and is the only consumer that pairs `popoutLeaf` + `popout_window`.
// ---------------------------------------------------------------------------

/**
 * Move `leaf` out of its current Tabs parent and into a fresh
 * FloatingWindow at the top of `floating[]`. Returns the new
 * FloatingWindow id so callers can pair it with the OS-side popout
 * window label.
 *
 * Layout effects:
 *  - The leaf is removed from its current Tabs (activeIndex clamped).
 *  - A new Tabs holding only this leaf is wrapped in a FloatingWindow
 *    and appended to `floating`.
 *  - The leaf's `parent` back-pointer is updated to the new Tabs.
 *  - `layout-change` fires once.
 *
 * No-op (returns the existing FW id) if the leaf is already inside a
 * FloatingWindow — calling popoutLeaf twice on the same leaf is a
 * benign double-click.
 *
 * Throws if the leaf is unknown to the store. Callers should pass a
 * `leafId` that came from `workspace.leaves.get(...)`.
 */
function popoutLeaf(
  leafId: string,
  bounds?: { x: number; y: number; w: number; h: number },
): string {
  const leaf = state().leaves.get(leafId)
  if (!leaf) {
    throw new Error(`[workspace.popoutLeaf] unknown leaf id: ${leafId}`)
  }
  // Already popped out — find the FW and return its id.
  const existing = state().floating.find((fw) =>
    findLeafAncestry(leaf, fw, []) !== null,
  )
  if (existing) {
    if (bounds) existing.bounds = bounds
    return existing.id
  }

  // Detach from current Tabs parent (without invoking view.onClose).
  const parent = leaf.parent
  if (parent && parent.kind === 'tabs') {
    const idx = parent.leaves.findIndex((l) => l.id === leaf.id)
    if (idx >= 0) {
      parent.leaves.splice(idx, 1)
      if (parent.activeIndex >= parent.leaves.length) {
        parent.activeIndex = Math.max(0, parent.leaves.length - 1)
      }
    }
  }

  const newTabs: Tabs = {
    kind: 'tabs',
    id: newId(),
    leaves: [leaf],
    activeIndex: 0,
  }
  const fw: FloatingWindow = {
    kind: 'floating',
    id: newId(),
    child: newTabs,
  }
  if (bounds) fw.bounds = bounds
  // Reparent the leaf so subsequent `setViewState` etc. report the
  // correct ancestry for tree walks.
  leaf.parent = newTabs

  useWorkspaceStore.setState({ floating: [...state().floating, fw] })
  emitInternal('layout-change')
  return fw.id
}

/**
 * Find a FloatingWindow by id. Returns null if absent.
 */
function findFloatingWindow(id: string): FloatingWindow | null {
  return state().floating.find((fw) => fw.id === id) ?? null
}

/**
 * Update the persisted bounds for a popout. Used by the Tauri bridge
 * after a window resize / move so the next `installAutoSave` debounce
 * captures the new bounds. No-op when the FW doesn't exist.
 */
function setFloatingWindowBounds(
  id: string,
  bounds: { x: number; y: number; w: number; h: number },
): void {
  const fw = findFloatingWindow(id)
  if (!fw) return
  // Skip the emit + save when nothing actually changed (drag end
  // sometimes fires a final event with the same bounds).
  if (
    fw.bounds &&
    fw.bounds.x === bounds.x &&
    fw.bounds.y === bounds.y &&
    fw.bounds.w === bounds.w &&
    fw.bounds.h === bounds.h
  ) {
    return
  }
  fw.bounds = { ...bounds }
  emitInternal('layout-change')
}

/**
 * Close a floating window. Detaches every leaf inside (running
 * `view.onClose`) and removes the window from `floating[]`. Idempotent
 * — closing an unknown id silently returns. The OS-side close call
 * lives in the Tauri bridge.
 */
async function closeFloatingWindow(id: string): Promise<void> {
  const fw = findFloatingWindow(id)
  if (!fw) return
  // Detach every leaf depth-first so views' onClose fires.
  const collectLeaves = (node: WorkspaceParent, out: Leaf[]): void => {
    if (node.kind === 'tabs') {
      out.push(...node.leaves)
      return
    }
    if (node.kind === 'split') {
      for (const c of node.children) collectLeaves(c, out)
      return
    }
    const withChild = node as { child?: WorkspaceParent }
    if (withChild.child) collectLeaves(withChild.child, out)
  }
  const leavesToDispose: Leaf[] = []
  collectLeaves(fw, leavesToDispose)
  for (const leaf of leavesToDispose) {
    await leaf.detach()
  }

  const remaining = state().floating.filter((other) => other.id !== fw.id)
  const nextLeaves = new Map(state().leaves)
  for (const leaf of leavesToDispose) {
    nextLeaves.delete(leaf.id)
  }
  const wasActive = leavesToDispose.some(
    (l) => state().activeLeafId === l.id,
  )
  useWorkspaceStore.setState({
    floating: remaining,
    leaves: nextLeaves,
    ...(wasActive ? { activeLeafId: null } : {}),
  })
  if (wasActive) emitInternal('active-leaf-change', { leaf: null })
  emitInternal('layout-change')
}

async function detachLeaf(leaf: Leaf): Promise<void> {
  // Plan decision #2 on order: await detach() first (triggers onClose,
  // clears the view) before tree mutation, so subscribers observing
  // `layout-change` never see a tree-attached leaf with a null view mid-flight.
  await leaf.detach()

  const parent = leaf.parent
  if (parent && parent.kind === 'tabs') {
    const idx = parent.leaves.findIndex(l => l.id === leaf.id)
    if (idx >= 0) {
      parent.leaves.splice(idx, 1)
      if (parent.activeIndex >= parent.leaves.length) {
        parent.activeIndex = Math.max(0, parent.leaves.length - 1)
      }
    }
  }

  const leaves = new Map(state().leaves)
  leaves.delete(leaf.id)
  const next: Partial<WorkspaceStoreState> = { leaves }
  const wasActive = state().activeLeafId === leaf.id
  // If the detached leaf was the active one, promote the next leaf
  // in its tabs group (post-splice) to active. This keeps activeLeafId
  // consistent with the tab strip's activeIndex and lets subscribers
  // (outline, status bar, editor plugin's active-leaf-change handler)
  // react to the tab change. If no such group exists or it is empty,
  // fall back to null — same as the pre-fix behaviour.
  let nextActive: Leaf | null = null
  if (wasActive && parent && parent.kind === 'tabs' && parent.leaves.length > 0) {
    nextActive = parent.leaves[parent.activeIndex] ?? parent.leaves[0] ?? null
  }
  if (wasActive) {
    next.activeLeafId = nextActive?.id ?? null
  }
  useWorkspaceStore.setState(next)

  // Fire active-leaf-change after the setState so subscribers see a
  // coherent store. Emit with the next leaf if one exists, otherwise
  // with an explicit null-leaf payload so listeners can clear (this
  // matches the shape the editor plugin already handles — a null
  // leaf short-circuits to clearing editorStore.activeRelpath).
  if (wasActive) {
    emitInternal('active-leaf-change', { leaf: nextActive })
  }

  emitInternal('layout-change')
}

function on(event: string, listener: Listener): () => void {
  const listeners = new Map(state().listeners)
  const existing = listeners.get(event) ?? new Set<Listener>()
  const nextSet = new Set(existing)
  nextSet.add(listener)
  listeners.set(event, nextSet)
  useWorkspaceStore.setState({ listeners })

  return () => {
    const cur = new Map(state().listeners)
    const bucket = cur.get(event)
    if (!bucket) return
    const nb = new Set(bucket)
    nb.delete(listener)
    if (nb.size === 0) cur.delete(event)
    else cur.set(event, nb)
    useWorkspaceStore.setState({ listeners: cur })
  }
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

function serializeNode(node: WorkspaceParent): SerializedNode {
  if (node.kind === 'tabs') {
    const out: SerializedTabs = {
      kind: 'tabs',
      id: node.id,
      leaves: node.leaves.map(l => ({
        kind: 'leaf',
        id: l.id,
        viewState: l.getViewState(),
      }) as SerializedLeaf),
      activeIndex: node.activeIndex,
    }
    return out
  }
  if (node.kind === 'split') {
    const out: SerializedSplit = {
      kind: 'split',
      id: node.id,
      direction: node.direction,
      children: node.children.map(serializeNode),
    }
    if (node.sizes) out.sizes = node.sizes
    if (isSidedock(node)) {
      out.side = node.side
      out.collapsed = node.collapsed
      out.size = node.size
    }
    return out
  }
  if (node.kind === 'root') {
    const out: SerializedRoot = {
      kind: 'root',
      id: node.id,
      child: serializeNode(node.child),
    }
    return out
  }
  // floating
  const out: SerializedFloating = {
    kind: 'floating',
    id: node.id,
    child: serializeNode(node.child),
  }
  if (node.bounds) out.bounds = node.bounds
  return out
}

function serialize(): WorkspaceJSON {
  const floatingSerialized: SerializedFloating[] = state().floating.map(
    (fw) => serializeNode(fw) as SerializedFloating,
  )
  return {
    main: serializeNode(state().rootSplit),
    left: serializeNode(state().leftSplit),
    right: serializeNode(state().rightSplit),
    bottom: serializeNode(state().bottomSplit),
    // Omit `floating` from the JSON when empty so the disk file stays
    // identical to pre-BL-029 output for users who never popped out a
    // leaf — keeps git diffs of `.forge/workspace.json` minimal in
    // existing forges.
    ...(floatingSerialized.length > 0 ? { floating: floatingSerialized } : {}),
    active: state().activeLeafId,
    lastOpenFiles: [], // Phase 6 will populate.
  }
}

// ---------------------------------------------------------------------------
// Hydrate
//
// Id preservation decision: leaf and node ids are preserved verbatim from
// the serialized form. Rationale: persistence/restore should round-trip
// exactly so that external references (cursor position by leaf id, command
// palette "focus leaf X") survive reload. Id regeneration would break that
// contract. Only nodes/leaves produced by the default layout (when json is
// absent) get fresh ids via crypto.randomUUID().
// ---------------------------------------------------------------------------

interface HydrateBuildResult {
  node: WorkspaceParent
  pendingLeaves: Array<{ leaf: Leaf; viewState: import('./types.ts').ViewState }>
}

function hydrateNode(
  serialized: SerializedNode,
  leaves: Map<string, Leaf>,
  pending: Array<{ leaf: Leaf; viewState: import('./types.ts').ViewState }>,
): WorkspaceParent {
  if (serialized.kind === 'tabs') {
    const tabs: Tabs = {
      kind: 'tabs',
      id: serialized.id,
      leaves: [],
      activeIndex: serialized.activeIndex,
    }
    for (const sLeaf of serialized.leaves) {
      const leaf = new LeafImpl(tabs, boundEmit)
      // Preserve serialized id — override the auto-generated one.
      ;(leaf as unknown as { id: string }).id = sLeaf.id
      leaves.set(leaf.id, leaf)
      tabs.leaves.push(leaf)
      pending.push({ leaf, viewState: sLeaf.viewState })
    }
    return tabs
  }
  if (serialized.kind === 'split') {
    const children = serialized.children.map(c => hydrateNode(c, leaves, pending))
    if (serialized.side !== undefined) {
      const dock: Sidedock = {
        kind: 'split',
        id: serialized.id,
        direction: serialized.direction,
        children,
        side: serialized.side,
        collapsed: serialized.collapsed ?? false,
        size: serialized.size ?? 300,
      }
      if (serialized.sizes) dock.sizes = serialized.sizes
      return dock
    }
    const split: Split = {
      kind: 'split',
      id: serialized.id,
      direction: serialized.direction,
      children,
    }
    if (serialized.sizes) split.sizes = serialized.sizes
    return split
  }
  if (serialized.kind === 'root') {
    return {
      kind: 'root',
      id: serialized.id,
      child: hydrateNode(serialized.child, leaves, pending),
    }
  }
  if (serialized.kind === 'floating') {
    const fw: FloatingWindow = {
      kind: 'floating',
      id: serialized.id,
      child: hydrateNode(serialized.child, leaves, pending),
    }
    if (serialized.bounds) fw.bounds = serialized.bounds
    return fw
  }
  // SerializedLeaf at top level is not valid — a leaf always lives inside Tabs.
  throw new Error(`[workspaceStore.hydrate] unexpected leaf node at structural position: ${JSON.stringify(serialized)}`)
}

function expectSplit(node: WorkspaceParent, label: string): Split {
  if (node.kind !== 'split') {
    throw new Error(`[workspaceStore.hydrate] ${label} must be a Split, got ${node.kind}`)
  }
  return node as Split
}

function expectSidedock(
  node: WorkspaceParent,
  side: 'left' | 'right' | 'bottom',
): Sidedock {
  const split = expectSplit(node, `${side} dock`)
  if (!isSidedock(split) || split.side !== side) {
    throw new Error(`[workspaceStore.hydrate] ${side} dock is not a Sidedock with side='${side}'`)
  }
  return split
}

/**
 * Build a fresh default bottom sidedock. Used during hydrate when the
 * persisted JSON has no `bottom` field (backwards-compat path for
 * workspace.json files written before the bottom drawer landed) and
 * during resetToDefault().
 */
function buildDefaultBottomDock(leaves: Map<string, Leaf>): Sidedock {
  const tabs = makeTabs()
  const dock: Sidedock = {
    kind: 'split',
    id: newId(),
    direction: 'horizontal',
    children: [tabs],
    side: 'bottom',
    collapsed: true,
    size: 240,
  }
  const leaf = new LeafImpl(tabs, boundEmit)
  leaves.set(leaf.id, leaf)
  tabs.leaves.push(leaf)
  void leaf.setViewState({ type: 'empty' })
  return dock
}

async function hydrate(json?: WorkspaceJSON): Promise<void> {
  if (!json) {
    resetToDefault()
    emitInternal('layout-ready')
    return
  }

  const leaves = new Map<string, Leaf>()
  const pending: Array<{ leaf: Leaf; viewState: import('./types.ts').ViewState }> = []

  const main = expectSplit(hydrateNode(json.main, leaves, pending), 'main')
  const left = expectSidedock(hydrateNode(json.left, leaves, pending), 'left')
  const right = expectSidedock(hydrateNode(json.right, leaves, pending), 'right')
  // Backwards-compat: older workspace.json files predate the bottom
  // drawer and have no `bottom` field. Seed a collapsed default so
  // existing users see no visual regression on first load after
  // upgrading.
  const bottom = json.bottom
    ? expectSidedock(hydrateNode(json.bottom, leaves, pending), 'bottom')
    : buildDefaultBottomDock(leaves)

  // BL-029 — restore popped-out leaves. Each SerializedFloating must
  // hydrate to a runtime FloatingWindow (not a generic node), so we
  // reuse `hydrateNode` and assert the result. A malformed entry
  // (anything that isn't a 'floating' node at top level) is dropped
  // with a warn so a single corrupt entry can't take down the whole
  // hydrate path.
  const floating: FloatingWindow[] = []
  if (json.floating && Array.isArray(json.floating)) {
    for (const fwJson of json.floating) {
      const node = hydrateNode(fwJson, leaves, pending)
      if (node.kind === 'floating') {
        floating.push(node)
      } else {
        console.warn(
          '[workspaceStore.hydrate] floating entry is not a floating node; skipped',
          fwJson,
        )
      }
    }
  }

  useWorkspaceStore.setState({
    rootSplit: main,
    leftSplit: left,
    rightSplit: right,
    bottomSplit: bottom,
    floating,
    activeLeafId: json.active,
    leaves,
  })

  // Drive every leaf through setViewState. Critical per plan Risks §:
  // `layout-ready` must fire AFTER every setViewState completes.
  for (const { leaf, viewState } of pending) {
    await leaf.setViewState(viewState)
  }

  emitInternal('layout-ready')
}

/** Install the default layout — called by hydrate(undefined) and tests. */
function resetToDefault(): void {
  const rootTabs = makeTabs()
  const rootSplit: Split = {
    kind: 'split',
    id: newId(),
    direction: 'horizontal',
    children: [rootTabs],
  }

  const leftTabs = makeTabs()
  const leftSplit: Sidedock = {
    kind: 'split',
    id: newId(),
    direction: 'vertical',
    children: [leftTabs],
    side: 'left',
    collapsed: false,
    size: 300,
  }

  const rightTabs = makeTabs()
  const rightSplit: Sidedock = {
    kind: 'split',
    id: newId(),
    direction: 'vertical',
    children: [rightTabs],
    side: 'right',
    collapsed: false,
    size: 300,
  }

  const bottomTabs = makeTabs()
  const bottomSplit: Sidedock = {
    kind: 'split',
    id: newId(),
    direction: 'horizontal',
    children: [bottomTabs],
    side: 'bottom',
    // Default-collapsed so first-run users don't see an empty drawer
    // eating vertical space under the main editor.
    collapsed: true,
    size: 240,
  }

  const leaves = new Map<string, Leaf>()

  const seed = (parent: Tabs): Leaf => {
    const leaf = new LeafImpl(parent, boundEmit)
    leaves.set(leaf.id, leaf)
    parent.leaves.push(leaf)
    return leaf
  }

  const rootLeaf = seed(rootTabs)
  seed(leftTabs)
  seed(rightTabs)
  seed(bottomTabs)

  useWorkspaceStore.setState({
    rootSplit,
    leftSplit,
    rightSplit,
    bottomSplit,
    floating: [],
    activeLeafId: null,
    leaves,
    listeners: state().listeners, // preserve subscribers across reset
  })

  // Fire-and-forget empty view state. Async but the default-layout path does
  // not await; empty.setState/onOpen are synchronous so this resolves
  // synchronously in practice.
  for (const leaf of leaves.values()) {
    void leaf.setViewState({ type: 'empty' })
  }
  // Mark the first root leaf as active by default for UX symmetry with
  // existing shell behavior.
  useWorkspaceStore.setState({ activeLeafId: rootLeaf.id })
}

// ---------------------------------------------------------------------------
// Non-reactive facade — singleton. Mirrors `slotRegistry` in
// shell/src/registry/SlotRegistry.ts:76-82.
// ---------------------------------------------------------------------------

export const workspace = {
  createLeaf,
  getLeftLeaf: (reveal?: boolean) => getSideLeaf('left', reveal),
  getRightLeaf: (reveal?: boolean) => getSideLeaf('right', reveal),
  ensureLeafOfType,
  revealLeaf,
  getLeavesOfType,
  setActiveLeaf,
  setSidedockSize,
  setSidedockCollapsed,
  setSplitSizes,
  setTabActiveIndex,
  detachLeaf,
  // BL-029
  popoutLeaf,
  closeFloatingWindow,
  setFloatingWindowBounds,
  findFloatingWindow,
  emit: emitInternal,
  on,
  serialize,
  hydrate,
  resetToDefault,
  // Raw accessors — handy for tests and future callers that want to walk
  // the tree without subscribing to Zustand directly.
  get rootSplit(): Split {
    return state().rootSplit
  },
  get leftSplit(): Sidedock {
    return state().leftSplit
  },
  get rightSplit(): Sidedock {
    return state().rightSplit
  },
  get bottomSplit(): Sidedock {
    return state().bottomSplit
  },
  get floating(): FloatingWindow[] {
    return state().floating
  },
  get activeLeafId(): string | null {
    return state().activeLeafId
  },
  get leaves(): Map<string, Leaf> {
    return state().leaves
  },
  // OI-14 — typed accessor for the active forge root, so plugins don't
  // need to reach into the forge zustand store directly. Returns null
  // between `workspace:closed` and the next `workspace:opened`.
  forgeRoot(): string | null {
    return useForgeStore.getState().rootPath
  },
}
