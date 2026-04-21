import { create } from 'zustand'

interface WorkspaceState {
  rootPath: string | null
  open: () => void
  setRootPath: (path: string | null) => void
  setOpenHandler: (handler: () => void) => void
}

export const useWorkspaceStore = create<WorkspaceState>((set) => ({
  rootPath: null,
  open: () => {},
  setRootPath: (path) => set({ rootPath: path }),
  setOpenHandler: (handler) => set({ open: handler }),
}))
