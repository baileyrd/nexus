// V16 — reactive forge-root accessor for host chrome (ForgeSelector,
// RightPanelFooter). Reads through the host-owned `WorkspaceHostSurface`
// seam instead of importing the workspace plugin's zustand store, so the
// dependency arrow stays plugin → host (see WorkspaceHostSurface.ts).
//
// `subscribeWorkspaceRootPath` handles the plugin registering its surface
// AFTER this hook mounts (chrome renders before plugin activation
// finishes): the subscription re-binds on registration and the snapshot
// is re-read. Until the plugin registers, the snapshot is `null` — the
// same "no forge open" value the plugin's own store starts with.

import { useSyncExternalStore } from 'react'
import {
  getWorkspaceRootPath,
  subscribeWorkspaceRootPath,
} from '../host/WorkspaceHostSurface'

/** Absolute path of the open forge root, or `null` when none is open
 *  (or the workspace plugin is absent). */
export function useWorkspaceRootPath(): string | null {
  return useSyncExternalStore(subscribeWorkspaceRootPath, getWorkspaceRootPath)
}
