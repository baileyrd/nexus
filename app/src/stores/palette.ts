import { create } from "zustand";

interface PaletteState {
  open: boolean;
  openPalette: () => void;
  closePalette: () => void;
  togglePalette: () => void;
}

/**
 * Controls the command palette modal. Both the global keybinding
 * (Cmd/Ctrl+K) and the `workspace.command-palette` command toggle the
 * modal through this store; the `<CommandPalette>` component reads
 * `open` to decide whether to render.
 */
export const usePaletteStore = create<PaletteState>((set) => ({
  open: false,
  openPalette: () => set({ open: true }),
  closePalette: () => set({ open: false }),
  togglePalette: () => set((s) => ({ open: !s.open })),
}));
