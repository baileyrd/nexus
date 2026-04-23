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
  /** UI preference for RAG retrieval. `ask` is always RAG-mode today;
   *  this flag exists so a future non-RAG direct-ask path can branch
   *  on it without another UI pass. Defaults on to match current
   *  behavior. */
  ragEnabled: boolean
  setInput: (s: string) => void
  appendMessage: (m: AiMessage) => void
  setSending: (b: boolean) => void
  setError: (e: string | null) => void
  setRagEnabled: (b: boolean) => void
  clear: () => void
}

export const useAiStore = create<AiState>((set) => ({
  messages: [],
  input: '',
  sending: false,
  error: null,
  ragEnabled: true,
  setInput: (s) => set({ input: s }),
  appendMessage: (m) => set((state) => ({ messages: [...state.messages, m] })),
  setSending: (b) => set({ sending: b }),
  setError: (e) => set({ error: e }),
  setRagEnabled: (b) => set({ ragEnabled: b }),
  clear: () =>
    set({ messages: [], input: '', sending: false, error: null, ragEnabled: true }),
}))
