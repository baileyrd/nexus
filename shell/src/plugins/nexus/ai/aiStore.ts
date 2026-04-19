import { create } from 'zustand'

export type AiRole = 'user' | 'assistant' | 'system' | 'error'

export interface AiSource {
  file_path: string
  block_id?: number
  excerpt?: string
  score?: number
}

export interface AiMessage {
  id: string
  role: AiRole
  /** Markdown for assistant; plain text for user/system/error. */
  content: string
  createdAtMs: number
  /** Retrieved RAG sources — only set for assistant messages. */
  sources?: AiSource[]
}

interface AiState {
  messages: AiMessage[]
  input: string
  sending: boolean
  error: string | null
  setInput: (s: string) => void
  appendMessage: (m: AiMessage) => void
  setSending: (b: boolean) => void
  setError: (e: string | null) => void
  clear: () => void
}

export const useAiStore = create<AiState>((set) => ({
  messages: [],
  input: '',
  sending: false,
  error: null,
  setInput: (s) => set({ input: s }),
  appendMessage: (m) => set((state) => ({ messages: [...state.messages, m] })),
  setSending: (b) => set({ sending: b }),
  setError: (e) => set({ error: e }),
  clear: () => set({ messages: [], input: '', sending: false, error: null }),
}))
