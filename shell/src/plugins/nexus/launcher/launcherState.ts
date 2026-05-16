import { invoke } from '@tauri-apps/api/core'
import { create } from 'zustand'
import { clientLogger } from '../../../clientLogger'

export interface RemoteForgeRecent {
  uri: string
  label?: string | null
}

export interface ShellState {
  version: number
  lastForgePath: string | null
  recentForgePaths: string[]
  remoteForgeRecents: RemoteForgeRecent[]
}

interface LauncherStore {
  recents: string[]
  remoteRecents: RemoteForgeRecent[]
  lastForgePath: string | null
  /** Set by the forge selector before calling `nexus.workspace.close`,
   *  so the launcher overlay can offer a "go back" close button that
   *  restores the forge the user was just inside of. Null on fresh
   *  boot (no forge to return to). Cleared on successful re-open. */
  manageReturnTo: string | null
  /** BL-148 — when non-null, the remote-connection modal is open. The
   *  string is the URI to prefill (used when "edit" lands; currently
   *  always empty for the create flow). */
  remoteModalOpen: boolean
  load: () => Promise<void>
  openPath: (path: string) => Promise<void>
  openRemote: (entry: RemoteForgeRecent) => Promise<void>
  forgetPath: (path: string) => Promise<void>
  forgetRemote: (uri: string) => Promise<void>
  setManageReturnTo: (path: string | null) => void
  setRemoteModalOpen: (open: boolean) => void
}

const EMPTY: ShellState = {
  version: 1,
  lastForgePath: null,
  recentForgePaths: [],
  remoteForgeRecents: [],
}

async function getState(): Promise<ShellState> {
  try {
    const raw = await invoke<ShellState>('get_shell_state')
    return {
      ...raw,
      remoteForgeRecents: raw.remoteForgeRecents ?? [],
    }
  } catch (err) {
    clientLogger.warn('[nexus.launcher] get_shell_state failed:', err)
    return EMPTY
  }
}

async function recordOpen(forgePath: string): Promise<ShellState> {
  return invoke<ShellState>('write_last_forge_path', { forgePath })
}

async function recordRemote(entry: RemoteForgeRecent): Promise<ShellState> {
  return invoke<ShellState>('write_remote_recent', {
    uri: entry.uri,
    label: entry.label ?? null,
  })
}

async function forget(forgePath: string): Promise<ShellState> {
  return invoke<ShellState>('forget_forge_path', { forgePath })
}

async function forgetRemoteUri(uri: string): Promise<ShellState> {
  return invoke<ShellState>('forget_remote_recent', { uri })
}

export const useLauncherStore = create<LauncherStore>((set) => ({
  recents: [],
  remoteRecents: [],
  lastForgePath: null,
  manageReturnTo: null,
  remoteModalOpen: false,
  async load() {
    const state = await getState()
    set({
      recents: state.recentForgePaths,
      remoteRecents: state.remoteForgeRecents ?? [],
      lastForgePath: state.lastForgePath,
    })
  },
  async openPath(path) {
    const state = await recordOpen(path)
    set({
      recents: state.recentForgePaths,
      remoteRecents: state.remoteForgeRecents ?? [],
      lastForgePath: state.lastForgePath,
      manageReturnTo: null,
    })
  },
  async openRemote(entry) {
    const state = await recordRemote(entry)
    set({
      recents: state.recentForgePaths,
      remoteRecents: state.remoteForgeRecents ?? [],
      lastForgePath: state.lastForgePath,
      manageReturnTo: null,
    })
  },
  async forgetPath(path) {
    const state = await forget(path)
    set({
      recents: state.recentForgePaths,
      remoteRecents: state.remoteForgeRecents ?? [],
      lastForgePath: state.lastForgePath,
    })
  },
  async forgetRemote(uri) {
    const state = await forgetRemoteUri(uri)
    set({
      recents: state.recentForgePaths,
      remoteRecents: state.remoteForgeRecents ?? [],
      lastForgePath: state.lastForgePath,
    })
  },
  setManageReturnTo(path) {
    set({ manageReturnTo: path })
  },
  setRemoteModalOpen(open) {
    set({ remoteModalOpen: open })
  },
}))
