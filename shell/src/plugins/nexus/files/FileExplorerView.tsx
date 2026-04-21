// Thin View wrapper for the Files tree (leaf-migration plan §Phase 5).
//
// The existing `FilesTree` React component owns its own state via
// `useFilesStore`, so this View is stateless — `getState`/`setState`
// use an empty shape. The plugin's store handles hydration across
// workspace switches already.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

/**
 * Render function supplied by the plugin's `activate` — captures the
 * `onFileActivate` closure so we don't need to thread the plugin API
 * through this module.
 */
type RenderFn = () => ReactElement

/**
 * ViewBase subclass that mounts `<FilesTree>` imperatively into the
 * leaf's container element. Matches Obsidian's `ItemView` lifecycle:
 * `onOpen(el)` plants a React root; `onClose` unmounts it.
 */
export class FileExplorerView extends ViewBase {
  readonly viewType = 'file-explorer'
  private root: Root | null = null
  private readonly render: RenderFn

  constructor(leaf: Leaf, render: RenderFn) {
    super(leaf)
    this.render = render
  }

  getState(): Record<string, never> {
    return {}
  }

  setState(_state: unknown): void {
    // Plugin state lives in `useFilesStore` — no per-View state to
    // rehydrate. Intentional no-op.
  }

  onOpen(el: HTMLElement): void {
    this.root = createRoot(el)
    this.root.render(this.render())
  }

  onClose(): void {
    this.root?.unmount()
    this.root = null
  }
}

/** Factory that returns a `ViewCreator` closing over the render fn. */
export function fileExplorerViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new FileExplorerView(leaf, render)
}
