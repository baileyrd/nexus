import { create } from 'zustand'

/**
 * One open file buffer. `relpath` is forge-relative, forward-slash
 * separated (matches the payload emitted by `nexus.files` on
 * `files:open`). `content` is the decoded UTF-8 text — or a sentinel
 * string when the bytes couldn't be decoded as UTF-8.
 */
export interface EditorOpenFile {
  relpath: string
  name: string
  content: string
}

interface EditorState {
  openFile: EditorOpenFile | null
  loading: boolean
  error: string | null
  setFile: (f: EditorOpenFile | null) => void
  setLoading: (b: boolean) => void
  setError: (s: string | null) => void
}

/**
 * Single-file read-only editor state. A second `files:open` event
 * replaces the buffer outright — tabs land in a follow-up commit.
 */
export const useEditorStore = create<EditorState>((set) => ({
  openFile: null,
  loading: false,
  error: null,
  setFile: (openFile) => set({ openFile }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
}))
