// src/plugins/core/terminal/terminalStore.ts
import { create } from 'zustand'

export interface TerminalLine {
  id: number
  text: string
  type: 'input' | 'output' | 'error' | 'system'
}

interface TerminalStore {
  lines: TerminalLine[]
  input: string
  counter: number
  addLine: (text: string, type: TerminalLine['type']) => void
  setInput: (input: string) => void
  clear: () => void
}

export const useTerminalStore = create<TerminalStore>((set) => ({
  lines: [{ id: 0, text: 'Shell terminal — type commands below', type: 'system' }],
  input: '',
  counter: 1,

  addLine: (text, type) =>
    set(s => ({
      lines: [...s.lines, { id: s.counter, text, type }],
      counter: s.counter + 1,
    })),

  setInput: (input) => set({ input }),

  clear: () => set({ lines: [], input: '' }),
}))
