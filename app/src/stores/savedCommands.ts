// Saved commands store (PRD-09 §14.1).
//
// Frontend-only snapshot of the procmgr_commands table that the Rust
// `SqliteSavedCommandStore` owns. Persisted to localStorage so users
// don't lose their list between sessions; a follow-up PRD-09 slice
// will wire this up to the `com.nexus.terminal` plugin once that
// crate exposes list/create/update/delete handlers.

import { create } from "zustand";

export interface SavedCommand {
  slug: string;
  name: string;
  shell: string;
  shellCmd: string;
  workingDir: string | null;
  icon: string;
}

interface SavedCommandsState {
  commands: SavedCommand[];
  add: (cmd: Omit<SavedCommand, "slug"> & { slug?: string }) => SavedCommand;
  update: (slug: string, patch: Partial<SavedCommand>) => void;
  remove: (slug: string) => void;
  reorder: (slug: string, direction: "up" | "down") => void;
}

const STORAGE_KEY = "nexus.saved-commands.v1";

function loadInitial(): SavedCommand[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isSavedCommand);
  } catch {
    return [];
  }
}

function isSavedCommand(v: unknown): v is SavedCommand {
  if (typeof v !== "object" || v === null) return false;
  const r = v as Record<string, unknown>;
  return (
    typeof r.slug === "string" &&
    typeof r.name === "string" &&
    typeof r.shell === "string" &&
    typeof r.shellCmd === "string" &&
    (r.workingDir === null || typeof r.workingDir === "string") &&
    typeof r.icon === "string"
  );
}

function persist(commands: SavedCommand[]): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(commands));
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn("[saved-commands] persist failed", err);
  }
}

function makeSlug(name: string, existing: string[]): string {
  const base = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/(^-|-$)/g, "")
    .slice(0, 48) || "cmd";
  if (!existing.includes(base)) return base;
  let n = 2;
  while (existing.includes(`${base}-${n}`)) n++;
  return `${base}-${n}`;
}

export const useSavedCommandsStore = create<SavedCommandsState>((set, get) => ({
  commands: loadInitial(),
  add: (cmd) => {
    const existing = get().commands.map((c) => c.slug);
    const slug = cmd.slug && !existing.includes(cmd.slug)
      ? cmd.slug
      : makeSlug(cmd.name, existing);
    const created: SavedCommand = {
      slug,
      name: cmd.name,
      shell: cmd.shell,
      shellCmd: cmd.shellCmd,
      workingDir: cmd.workingDir ?? null,
      icon: cmd.icon || "terminal",
    };
    const next = [...get().commands, created];
    persist(next);
    set({ commands: next });
    return created;
  },
  update: (slug, patch) => {
    const next = get().commands.map((c) =>
      c.slug === slug ? { ...c, ...patch, slug: c.slug } : c,
    );
    persist(next);
    set({ commands: next });
  },
  remove: (slug) => {
    const next = get().commands.filter((c) => c.slug !== slug);
    persist(next);
    set({ commands: next });
  },
  reorder: (slug, direction) => {
    const list = get().commands;
    const idx = list.findIndex((c) => c.slug === slug);
    if (idx < 0) return;
    const swap = direction === "up" ? idx - 1 : idx + 1;
    if (swap < 0 || swap >= list.length) return;
    const next = list.slice();
    [next[idx], next[swap]] = [next[swap]!, next[idx]!];
    persist(next);
    set({ commands: next });
  },
}));
