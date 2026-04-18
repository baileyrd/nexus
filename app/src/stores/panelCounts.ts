import { create } from "zustand";

/**
 * Panel-to-count registry read by the Inspector tab labels.
 *
 * Panel components that expose a meaningful count (Outline → heading
 * count, Backlinks → backlink count) call `setPanelCount(panel.id, n)`
 * on each render. The PanelSelector reads via `usePanelCount(id)` and
 * renders it inline with the title (e.g. "Outline 14").
 *
 * This sidesteps a `count` field on the Panel Rust type: panels own
 * their count the same way they own their content, the shell just
 * subscribes to display it. When a panel has no registered count,
 * callers see `undefined` and omit the badge.
 */
interface PanelCountsState {
  counts: Record<string, number>;
  setPanelCount: (panelId: string, count: number | null) => void;
}

export const usePanelCountsStore = create<PanelCountsState>((set) => ({
  counts: {},
  setPanelCount: (panelId, count) =>
    set((s) => {
      // Short-circuit identical writes so React doesn't re-render
      // every panel selector on every Outline parse pass.
      if (count === null) {
        if (!(panelId in s.counts)) return s;
        const next = { ...s.counts };
        delete next[panelId];
        return { counts: next };
      }
      if (s.counts[panelId] === count) return s;
      return { counts: { ...s.counts, [panelId]: count } };
    }),
}));

/** Hook: read the count for one panel. `undefined` when unknown. */
export function usePanelCount(panelId: string): number | undefined {
  return usePanelCountsStore((s) => s.counts[panelId]);
}
