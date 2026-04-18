// Saved commands store (PRD-09 §14.1).
//
// Thin IPC-backed cache over `com.nexus.terminal`'s `saved_*` handlers,
// which persist to `{forge}/.forge/procmgr.sqlite` via
// `nexus_terminal::SqliteSavedCommandStore`. This file maps the plugin's
// snake_case DTO to the camelCase shape the UI uses.
//
// The panel calls `load()` on mount; every CRUD action writes through to
// the plugin and then refreshes the cached list from the response so the
// UI never drifts from the backing DB.

import { create } from "zustand";

import {
  termSavedList,
  termSavedCreate,
  termSavedUpdate,
  termSavedDelete,
  termSavedReorder,
  type SavedCommandDto,
} from "../ipc/terminal";

/** Canonical frontend shape — camelCase view of `SavedCommandDto`. */
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
  loaded: boolean;
  loadError: string | null;
  load: () => Promise<void>;
  add: (
    cmd: Omit<SavedCommand, "slug"> & { slug?: string },
  ) => Promise<SavedCommand>;
  update: (slug: string, patch: Partial<SavedCommand>) => Promise<void>;
  remove: (slug: string) => Promise<void>;
  reorder: (slug: string, direction: "up" | "down") => Promise<void>;
}

function fromDto(d: SavedCommandDto): SavedCommand {
  return {
    slug: d.slug,
    name: d.name,
    shell: d.shell,
    shellCmd: d.shell_cmd,
    workingDir: d.working_dir,
    icon: d.icon,
  };
}

function toDto(c: SavedCommand, existing?: SavedCommandDto): SavedCommandDto {
  const now = Math.floor(Date.now() / 1000);
  return {
    slug: c.slug,
    name: c.name,
    shell: c.shell,
    shell_cmd: c.shellCmd,
    working_dir: c.workingDir,
    env_vars: existing?.env_vars ?? {},
    env_file: existing?.env_file ?? null,
    icon: c.icon || "terminal",
    auto_restart: existing?.auto_restart ?? false,
    auto_restart_delay_ms: existing?.auto_restart_delay_ms ?? 2000,
    memory_limit_mb: existing?.memory_limit_mb ?? null,
    sidebar_order: existing?.sidebar_order ?? null,
    pre_commands: existing?.pre_commands ?? [],
    created_at: existing?.created_at ?? now,
    updated_at: now,
  };
}

function makeSlug(name: string, existing: string[]): string {
  const base =
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/(^-|-$)/g, "")
      .slice(0, 48) || "cmd";
  if (!existing.includes(base)) return base;
  let n = 2;
  while (existing.includes(`${base}-${n}`)) n++;
  return `${base}-${n}`;
}

/** Remember the last-seen DTO per slug so updates preserve fields the UI
 *  doesn't surface (env_vars, pre_commands, timestamps). */
const dtoCache = new Map<string, SavedCommandDto>();

export const useSavedCommandsStore = create<SavedCommandsState>((set, get) => ({
  commands: [],
  loaded: false,
  loadError: null,

  load: async () => {
    try {
      const rows = await termSavedList();
      dtoCache.clear();
      for (const r of rows) dtoCache.set(r.slug, r);
      set({
        commands: rows.map(fromDto),
        loaded: true,
        loadError: null,
      });
    } catch (err) {
      set({ loaded: true, loadError: String(err) });
    }
  },

  add: async (cmd) => {
    const existing = Array.from(dtoCache.keys());
    const slug =
      cmd.slug && !existing.includes(cmd.slug)
        ? cmd.slug
        : makeSlug(cmd.name, existing);
    const candidate: SavedCommand = {
      slug,
      name: cmd.name,
      shell: cmd.shell,
      shellCmd: cmd.shellCmd,
      workingDir: cmd.workingDir ?? null,
      icon: cmd.icon || "terminal",
    };
    const created = await termSavedCreate(toDto(candidate));
    dtoCache.set(created.slug, created);
    set({ commands: [...get().commands, fromDto(created)] });
    return fromDto(created);
  },

  update: async (slug, patch) => {
    const current = get().commands.find((c) => c.slug === slug);
    if (!current) return;
    const merged: SavedCommand = {
      ...current,
      ...patch,
      slug: current.slug,
    };
    const fresh = await termSavedUpdate(toDto(merged, dtoCache.get(slug)));
    dtoCache.set(fresh.slug, fresh);
    set({
      commands: get().commands.map((c) =>
        c.slug === slug ? fromDto(fresh) : c,
      ),
    });
  },

  remove: async (slug) => {
    await termSavedDelete(slug);
    dtoCache.delete(slug);
    set({ commands: get().commands.filter((c) => c.slug !== slug) });
  },

  reorder: async (slug, direction) => {
    const list = get().commands;
    const idx = list.findIndex((c) => c.slug === slug);
    if (idx < 0) return;
    const swapIdx = direction === "up" ? idx - 1 : idx + 1;
    if (swapIdx < 0 || swapIdx >= list.length) return;
    // Persist via sidebar_order: assign fresh ordinals based on the new
    // positions. Keeps gaps shallow so later inserts don't need to
    // compact the whole column.
    const next = list.slice();
    [next[idx], next[swapIdx]] = [next[swapIdx]!, next[idx]!];
    for (let i = 0; i < next.length; i++) {
      const c = next[i]!;
      await termSavedReorder(c.slug, i);
      const cached = dtoCache.get(c.slug);
      if (cached) cached.sidebar_order = i;
    }
    set({ commands: next });
  },
}));
