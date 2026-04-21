// Type scaffolding for the Leaf + ViewRegistry workspace model.
// Mirrors Obsidian's Workspace/WorkspaceLeaf/ViewRegistry semantics.
// Source of truth: /home/baileyrd/projects/obsidian_reverse/docs/10-editor-shell.md §§1–4.
// This file is pure types — no runtime values, no imports from other Nexus code.

export interface View {
  readonly viewType: string
  readonly leaf: Leaf
  getState(): unknown
  setState(state: unknown, eState?: unknown): Promise<void> | void
  onOpen(containerEl: HTMLElement): Promise<void> | void
  onClose(): Promise<void> | void
}

export type ViewCreator = (leaf: Leaf) => View

// Matches Obsidian's ViewState exactly — see docs/10-editor-shell.md §3.
export interface ViewState {
  type: string
  state?: unknown
  active?: boolean
  pinned?: boolean
  group?: string
}

export interface Leaf {
  readonly id: string
  parent: WorkspaceParent
  view: View | null
  containerEl: HTMLElement | null
  pinned: boolean
  group: string | null
  setViewState(state: ViewState, eState?: unknown): Promise<void>
  getViewState(): ViewState
  detach(): Promise<void>
}

export type WorkspaceParent = Split | Tabs | Sidedock | Root | FloatingWindow

export interface Split {
  kind: 'split'
  id: string
  direction: 'horizontal' | 'vertical'
  children: WorkspaceParent[]
  sizes?: number[]
}

export interface Tabs {
  kind: 'tabs'
  id: string
  leaves: Leaf[]
  activeIndex: number
}

// Sidedock IS a Split with side metadata — mirrors Obsidian's FD extends OD
// (see docs/10-editor-shell.md §2). Keeping `kind: 'split'` means tree walkers
// that switch on `kind` handle sidedocks uniformly; the `side` field is the
// only discriminator between a generic split and a dock.
export interface Sidedock extends Split {
  kind: 'split'
  side: 'left' | 'right'
  collapsed: boolean
  size: number
}

export interface Root {
  kind: 'root'
  id: string
  child: WorkspaceParent
}

export interface FloatingWindow {
  kind: 'floating'
  id: string
  child: WorkspaceParent
  bounds?: { x: number; y: number; w: number; h: number }
}

// Persistence shape — JSON-safe mirror of the runtime tree.
// No Leaf/HTMLElement references; every node carries only serializable fields.
// Matches the format in leaf-migration-plan.md §Phase 6 for forward-compat with Obsidian.

export interface SerializedSplit {
  kind: 'split'
  id: string
  direction: 'horizontal' | 'vertical'
  children: SerializedNode[]
  sizes?: number[]
  // Sidedock-only fields; present when this split is a dock.
  side?: 'left' | 'right'
  collapsed?: boolean
  size?: number
}

export interface SerializedTabs {
  kind: 'tabs'
  id: string
  leaves: SerializedLeaf[]
  activeIndex: number
}

export interface SerializedLeaf {
  kind: 'leaf'
  id: string
  viewState: ViewState
}

export interface SerializedRoot {
  kind: 'root'
  id: string
  child: SerializedNode
}

export interface SerializedFloating {
  kind: 'floating'
  id: string
  child: SerializedNode
  bounds?: { x: number; y: number; w: number; h: number }
}

export type SerializedNode =
  | SerializedSplit
  | SerializedTabs
  | SerializedLeaf
  | SerializedRoot
  | SerializedFloating

export interface WorkspaceJSON {
  main: SerializedNode
  left: SerializedNode
  right: SerializedNode
  active: string | null
  lastOpenFiles: string[]
}
