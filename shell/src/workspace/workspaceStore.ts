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
  return {
    main: serializeNode(state().rootSplit),
    left: serializeNode(state().leftSplit),
    right: serializeNode(state().rightSplit),
    bottom: serializeNode(state().bottomSplit),
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

  useWorkspaceStore.setState({
    rootSplit: main,
    leftSplit: left,
    rightSplit: right,
    bottomSplit: bottom,
    floating: [],
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
  setTabActiveIndex,
  detachLeaf,
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
}
