// useWorkspaceField — reactive read over a field of the (mutated-in-place)
// workspace tree.
//
// Same recipe as `useLayoutVersion` in WorkspaceRenderer.tsx: subscribe to
// `layout-change` and force a re-render. The `read` closure runs on every
// render and reads the live workspace tree imperatively, so callers always
// see the current value without relying on Zustand selector identity
// (which never changes when nodes are mutated in place).
//
// Typical usage:
//   const sidebarCollapsed = useWorkspaceField(() => workspace.leftSplit.collapsed)

import { useEffect, useReducer } from 'react'
import { workspace } from './workspaceStore.ts'

export function useWorkspaceField<T>(read: () => T): T {
  const [, force] = useReducer((x: number) => x + 1, 0)
  useEffect(() => {
    const off = workspace.on('layout-change', () => force())
    return off
  }, [])
  return read()
}
