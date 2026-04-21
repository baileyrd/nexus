// Abstract base class for Nexus Views.
// Convenience for plugin authors — mirrors Obsidian's `ItemView` noop defaults
// (see /home/baileyrd/projects/obsidian_reverse/docs/10-editor-shell.md §3).
// Subclasses override only the hooks they need.

import type { Leaf, View } from './types.ts'

/**
 * Minimal base class implementing the {@link View} interface.
 *
 * Every subclass must set a concrete `viewType`. All lifecycle methods
 * (`getState`, `setState`, `onOpen`, `onClose`) have no-op defaults so
 * trivial views need not implement them.
 */
export abstract class ViewBase implements View {
  abstract readonly viewType: string
  readonly leaf: Leaf

  constructor(leaf: Leaf) {
    this.leaf = leaf
  }

  getState(): unknown {
    return {}
  }

  setState(_state: unknown, _eState?: unknown): Promise<void> | void {
    // default no-op — subclasses override to hydrate view-specific state
  }

  onOpen(_containerEl: HTMLElement): Promise<void> | void {
    // default no-op — subclasses override to mount DOM into _containerEl
  }

  onClose(): Promise<void> | void {
    // default no-op — subclasses override to tear down resources
  }
}
