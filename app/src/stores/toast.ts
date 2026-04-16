import { create } from "zustand";

export type ToastLevel = "info" | "warn" | "error";

export interface Toast {
  id: string;
  level: ToastLevel;
  message: string;
  /** Reverse-DNS plugin id that issued the notification, if any. */
  source?: string;
}

interface ToastState {
  toasts: Toast[];
  add(opts: { level: ToastLevel; message: string; source?: string }): string;
  remove(id: string): void;
}

let _seq = 0;

const AUTO_DISMISS_MS = 5000;

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],

  add({ level, message, source }) {
    const id = `toast-${++_seq}`;
    set((s) => ({ toasts: [...s.toasts, { id, level, message, source }] }));
    setTimeout(() => {
      useToastStore.getState().remove(id);
    }, AUTO_DISMISS_MS);
    return id;
  },

  remove(id) {
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },
}));
