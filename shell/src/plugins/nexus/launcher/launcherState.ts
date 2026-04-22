import { invoke } from '@tauri-apps/api/core'
import { create } from 'zustand'

export interface ShellState {
  version: number
  lastForgePath: string | null
  recentForgePaths: string[]
}

interface LauncherStore {
  recents: string[]
  lastForgePath: string | null
  /** Set by the forge selector before calling `nexus.workspace.close`,
   *  so the launcher overlay can offer a "go back" close button that
   *  restores the forge the user was just inside of. Null on fresh
   *  boot (no forge to return to). Cleared on successful re-open. */
  manageReturnTo: string | null
  load: () => Promise<void>
  openPath: (path: string) => Promise<void>
  forgetPath: (path: string) => Promise<void>
  setManageReturnTo: (path: string | null) => void
}

const EMPTY: ShellState = { version: 1, lastForgePath: null, recentForgePaths: [] }

async function getState(): Promise<ShellState> {
  try {
    return await invoke<ShellState>('get_shell_state')
  } catch (err) {
    console.warn('[nexus.launcher] get_shell_state failed:', err)
    return EMPTY
  }
}

async function recordOpen(forgePath: string): Promise<ShellState> {
  return invoke<ShellState>('write_last_forge_path', { forgePath })
}

async function forget(forgePath: string): Promise<ShellState> {
  return invoke<ShellState>('forget_forge_path', { forgePath })
}

export const useLauncherStore = create<LauncherStore>((set) => ({
  recents: [],
  lastForgePath: null,
  manageReturnTo: null,
  async load() {
    const state = await getState()
    set({ recents: state.recentForgePaths, lastForgePath: state.lastForgePath })
  },
  async openPath(path) {
    const state = await recordOpen(path)
    set({
      recents: state.recentForgePaths,
      lastForgePath: state.lastForgePath,
      manageReturnTo: null,
    })
  },
  async forgetPath(path) {
    const state = await forget(path)
    set({ recents: state.recentForgePaths, lastForgePath: state.lastForgePath })
  },
  setManageReturnTo(path) {
    set({ manageReturnTo: path })
  },
}))
