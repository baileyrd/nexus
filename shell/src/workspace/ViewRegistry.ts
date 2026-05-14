// Registry of view-type creators and file-extension → viewType bindings.
// Zustand-backed so future UI surfaces can react to registrations; use the
// `viewRegistry` singleton for non-reactive access outside React.

import { create } from 'zustand'
import type { Leaf, View, ViewCreator } from './types'
import { clientLogger } from '../host/clientLogger'

interface ViewStore {
  creators: Map<string, ViewCreator>
  extensions: Map<string, string>
  register: (type: string, creator: ViewCreator) => () => void
  /** Silently overwrite a creator — for intentional upgrades (e.g. bootstrap → full UI). */
  update: (type: string, creator: ViewCreator) => () => void
  registerExtensions: (exts: string[], type: string) => () => void
  getCreator: (type: string) => ViewCreator | null
  getTypeForExt: (ext: string) => string | null
}

export const useViewStore = create<ViewStore>((set, get) => ({
  creators: new Map(),
  extensions: new Map(),

  register: (type, creator) => {
    set(s => {
      if (s.creators.has(type)) {
        clientLogger.warn(`[ViewRegistry] overriding existing creator for '${type}'`)
      }
      const creators = new Map(s.creators)
      creators.set(type, creator)
      return { creators }
    })
    return () => {
      set(s => {
        // Only remove if the creator we registered is still the one registered.
        if (s.creators.get(type) !== creator) return s
        const creators = new Map(s.creators)
        creators.delete(type)
        return { creators }
      })
    }
  },

  update: (type, creator) => {
    set(s => {
      const creators = new Map(s.creators)
      creators.set(type, creator)
      return { creators }
    })
    return () => {
      set(s => {
        if (s.creators.get(type) !== creator) return s
        const creators = new Map(s.creators)
        creators.delete(type)
        return { creators }
      })
    }
  },

  registerExtensions: (exts, type) => {
    set(s => {
      const extensions = new Map(s.extensions)
      for (const ext of exts) extensions.set(ext, type)
      return { extensions }
    })
    return () => {
      set(s => {
        const extensions = new Map(s.extensions)
        // Only remove mappings still pointing at the type this call registered.
        for (const ext of exts) {
          if (extensions.get(ext) === type) extensions.delete(ext)
        }
        return { extensions }
      })
    }
  },

  getCreator: (type) => get().creators.get(type) ?? null,
  getTypeForExt: (ext) => get().extensions.get(ext) ?? null,
}))

// Non-reactive facade — for use outside React (plugin host, hydrate path).
export const viewRegistry = {
  register: (type: string, creator: ViewCreator) =>
    useViewStore.getState().register(type, creator),

  /** Silently overwrite a creator without the overwrite warning. Use when
   *  upgrading a bootstrap placeholder to a full implementation. */
  update: (type: string, creator: ViewCreator) =>
    useViewStore.getState().update(type, creator),

  registerExtensions: (exts: string[], type: string) =>
    useViewStore.getState().registerExtensions(exts, type),

  getCreator: (type: string) => useViewStore.getState().getCreator(type),

  getTypeForExt: (ext: string) => useViewStore.getState().getTypeForExt(ext),

  /**
   * BL-067 Phase 0 — list every registered view type. The View
   * Builder's "Add panel" palette consumes this to populate its
   * picker. The actual creators aren't surfaced (they're not
   * serialisable + the builder UI doesn't invoke them directly).
   */
  registeredTypes(): string[] {
    return [...useViewStore.getState().creators.keys()].sort()
  },

  /**
   * BL-067 Phase 0 — list every `extension → viewType` binding so
   * the builder can surface "files of type .md open as the editor
   * view" affordances.
   */
  registeredExtensions(): Array<{ extension: string; viewType: string }> {
    const out: Array<{ extension: string; viewType: string }> = []
    for (const [ext, type] of useViewStore.getState().extensions) {
      out.push({ extension: ext, viewType: type })
    }
    out.sort((a, b) => a.extension.localeCompare(b.extension))
    return out
  },
}

// Built-in `empty` view — legal persisted state per leaf-migration-plan §Phase 1.
const createEmptyView: ViewCreator = (leaf: Leaf): View => ({
  viewType: 'empty',
  leaf,
  getState: () => ({}),
  setState: () => {},
  onOpen: () => {},
  onClose: () => {},
})

// Guard against duplicate registration on HMR / repeated module evaluation.
declare global {
   
  var __nexusEmptyViewRegistered: boolean | undefined
}

if (!globalThis.__nexusEmptyViewRegistered) {
  useViewStore.getState().register('empty', createEmptyView)
  globalThis.__nexusEmptyViewRegistered = true
}
