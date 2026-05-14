// shell/src/host/layoutSnapshot.ts
//
// BL-067 Phase 0 — layout introspection API.
//
// Produces a JSON-safe snapshot of the shell's contribution layout
// at a point in time: every slot's entries (chrome positions like
// titleBar / activityBar / statusBar), every registered view-type
// creator (sources of leaf content), the live workspace tree
// (where leaves currently sit), and the view-type → owning-plugin
// mapping. The View Builder UI (BL-067 Phase 1+) reads this to
// render its inventory + composition canvas.
//
// Pure aggregation — never mutates state. The snapshot is a
// point-in-time projection; callers re-take it after a layout
// change (the workspace `layout-change` event is the natural
// trigger).

import { slotRegistry, type SlotEntrySnapshot, type SlotInventory } from '../registry/SlotRegistry'
import { viewRegistry } from '../workspace/ViewRegistry'
import { workspace } from '../workspace/workspaceStore'
import type { WorkspaceJSON } from '../workspace/types'
import type { PluginRegistry } from './PluginRegistry'

/** One registered `viewType` plus its owning plugin (when known).
 *  Built-in types like `empty` and `markdown` are owned by the
 *  shell itself; their `pluginId` is `null`. */
export interface ViewTypeSnapshot {
  /** The view type identifier (e.g. `markdown`, `nexus.outline.view`). */
  type: string
  /** Reverse-DNS id of the plugin that registered the creator, or
   *  `null` for shell built-ins (the `empty` view). */
  pluginId: string | null
}

/** One file-extension binding (`md` → `markdown` etc.). */
export interface ViewExtensionSnapshot {
  extension: string
  viewType: string
}

/**
 * Top-level layout introspection result. JSON-safe end-to-end so
 * a caller can `JSON.stringify` it for a "Save as layout" round-trip
 * or feed it into a builder's diffing logic.
 */
export interface LayoutSnapshot {
  /** Chrome-slot contributions — keyed by `SlotId`. The shape
   *  preserves the registry's priority ordering. */
  slots: SlotInventory
  /** Every registered view type with its owning plugin. */
  viewTypes: ViewTypeSnapshot[]
  /** `extension → viewType` bindings. */
  extensions: ViewExtensionSnapshot[]
  /** Live workspace tree — the same shape `.forge/workspace.json`
   *  persists, sourced from `workspace.layoutSnapshot()`. */
  layout: WorkspaceJSON
  /** Wall-clock millisecond when the snapshot was taken. Useful
   *  for diff timestamps when the builder compares two takes. */
  takenAtMs: number
}

/**
 * Build a [`LayoutSnapshot`] from the live registries.
 *
 * `pluginRegistry` is the source of truth for view-type ownership
 * (which plugin called `viewRegistry.register`). Optional —
 * callers without a registry handy get `null` pluginIds for every
 * view type. Production callers pass the singleton constructed in
 * `shellHost.ts`; tests can pass a mock or omit it entirely.
 */
export function getLayoutSnapshot(pluginRegistry?: PluginRegistry): LayoutSnapshot {
  const slots = slotRegistry.snapshot()
  const viewTypes: ViewTypeSnapshot[] = viewRegistry.registeredTypes().map((type) => ({
    type,
    pluginId: pluginRegistry?.ownerOfViewType(type) ?? null,
  }))
  const extensions = viewRegistry.registeredExtensions()
  const layout = workspace.layoutSnapshot()
  return {
    slots,
    viewTypes,
    extensions,
    layout,
    takenAtMs: Date.now(),
  }
}

/**
 * Process-global handle the shell host wires at boot. Plugin code
 * that wants a complete snapshot (with view-type ownership) calls
 * [`globalSnapshot`] which routes through this handle.
 *
 * Kept as a mutable module-level slot rather than a parameter
 * because every plugin call site would otherwise need to thread
 * `pluginRegistry` down through a half-dozen layers — the
 * registry is a true singleton in production.
 */
let registrySlot: PluginRegistry | null = null

/** Capture the plugin registry singleton; called once from
 *  `shellHost.ts` at boot. */
export function bindPluginRegistry(registry: PluginRegistry): void {
  registrySlot = registry
}

/** Convenience accessor for plugin code — same shape as
 *  [`getLayoutSnapshot`] but uses the bound registry from
 *  [`bindPluginRegistry`]. */
export function globalSnapshot(): LayoutSnapshot {
  return getLayoutSnapshot(registrySlot ?? undefined)
}

/**
 * Count every leaf in a workspace layout. Useful for the
 * builder's status line ("12 leaves across 3 splits + 1 floating
 * window"). Delegated here so callers don't re-walk the tree.
 */
export function countLeavesInLayout(json: WorkspaceJSON): number {
  let n = 0
  const walk = (node: unknown): void => {
    if (!node || typeof node !== 'object') return
    const obj = node as { kind?: string; children?: unknown[]; child?: unknown }
    if (obj.kind === 'leaf') {
      n += 1
      return
    }
    if (Array.isArray(obj.children)) {
      for (const c of obj.children) walk(c)
    }
    if (obj.child) walk(obj.child)
    if (obj.kind === 'tabs') {
      const leaves = (node as { leaves?: unknown[] }).leaves
      if (Array.isArray(leaves)) for (const l of leaves) walk(l)
    }
  }
  walk(json.main)
  walk(json.left)
  walk(json.right)
  walk(json.bottom)
  for (const fw of json.floating ?? []) walk(fw)
  return n
}

/** Re-export the constituent types so plugins consuming the
 *  snapshot only need one import. */
export type { SlotInventory, SlotEntrySnapshot }
