// Per-leaf state for an open `.bases` file. Phase 1 tracks only the
// loaded `Base` plus a load/error gate; later phases add active view,
// selection, pending edits, and an undo stack (mirrors canvasStore).

import { create } from 'zustand'
import { UNDO_HISTORY_CAP } from '../constants'
import type { Base, BaseRecord, BaseView, FilterRule } from './kernelClient'

export type ViewMode = 'table' | 'board' | 'list' | 'calendar' | 'gallery' | 'timeline'

export type SortDir = 'asc' | 'desc'

export interface SortState {
  field: string
  dir: SortDir
}

export interface BasesTabState {
  base: Base | null
  loading: boolean
  error: string | null
  /** Name of the active view, or null to fall back to a synthetic
   *  default Table view of every field. Phase 5 persists this. */
  activeView: string | null
  /** Client-side sort applied on top of the kernel records. Phase 2
   *  supports single-column sort; Phase 5 pushes this into view
   *  config so it round-trips to the `.bases` file. */
  sort: SortState | null
  /** Currently selected record id, or null. Used for keyboard
   *  navigation and the row-delete action. */
  selectedRecordId: string | null
  /** Current view mode. Phase-3 addition — in-memory only; Phase 5
   *  wires persistence via `base_view_*`. */
  viewMode: ViewMode
  /** User-picked group field for the Board view (must be `select`).
   *  Null = auto-detect first `select` field. */
  boardGroupField: string | null
  /** User-picked group field for the List view (any field). Null =
   *  fall back to the primary (or first) field. */
  listGroupField: string | null
  /** Set of collapsed group keys in the List view. */
  collapsedGroups: Set<string>
  /** Calendar view — field whose value is plotted as a date. Null =
   *  auto-detect the first `date`/`datetime` field. */
  calendarDateField: string | null
  /** Calendar view — first-of-month being displayed (ISO `yyyy-mm`). */
  calendarMonth: string | null
  /** Gallery view — field that drives the card cover. Null =
   *  auto-detect the first `url` field (treated as image). */
  galleryImageField: string | null
  /** Timeline view — select field whose distinct values become
   *  swimlanes. Null = auto-detect the first `select` field. */
  timelineGroupField: string | null
  timelineStartField: string | null
  timelineEndField: string | null
  /** Pixels per day in the timeline. Clamped [2, 80]. */
  timelineDayPx: number
  /** Whether the schema-editor side panel is visible for this tab. */
  schemaEditorOpen: boolean
  /** Trash mode — when true, the visible-records filter inverts:
   *  views show ONLY soft-deleted records and the row actions surface
   *  Restore + "Delete forever" instead of soft-delete. WI-10 §4.2
   *  acceptance ("records can be restored via UI"). */
  trashOpen: boolean
  /** Per-view hidden columns — names that round-trip via the
   *  `BaseView.fields` allowlist. Empty/null = show every column.
   *  Phase 5 round-trip fix. */
  hiddenFields: string[] | null
  /** Per-view filter chips — round-trip via `BaseView.filter`.
   *  Empty array = no filter; the shell narrows visible records by
   *  AND-ing every rule. Phase 5 round-trip fix. */
  viewFilters: FilterRule[]
  /** Undo stack — LIFO. Each entry owns a `forward` + `inverse`
   *  pair; `forward` was already applied when the entry was pushed,
   *  so `undo()` runs `inverse` and moves the entry to `redoStack`. */
  undoStack: HistoryEntry[]
  /** Redo stack — cleared whenever a fresh user edit is pushed. */
  redoStack: HistoryEntry[]
  /** When true, mutation actions are not exposed in the UI. Set for
   *  Obsidian single-file `.base` tabs (ADR 0019) — Nexus's `.bases`
   *  directory format remains read/write. */
  readOnly: boolean
  /** Filter expressions that fell outside the v1 evaluator grammar.
   *  Empty on the happy path; surfaced as a banner above the body
   *  when non-empty. ADR 0019. */
  unsupportedFilters: string[]
  /** Last error from `undo` / `redo` (e.g. "history truncated" when a
   *  destructive schema mutation refused to push). `pushHistory`
   *  clears this on the next push so it stays scoped to the most
   *  recent failure. Surfaced as a dismissable banner. */
  lastUndoError: string | null
}

/** One atomic user-visible edit. `forward`/`inverse` are async
 *  closures that call through the kernel client and patch local
 *  state. The label surfaces in tooltips/undo menus. */
export interface HistoryEntry {
  label: string
  forward(): Promise<void>
  inverse(): Promise<void>
}

const TIMELINE_DAY_PX_MIN = 2
const TIMELINE_DAY_PX_MAX = 80

interface BasesStore {
  /** Keyed by the leaf's relpath — one tab per open base. */
  tabs: Record<string, BasesTabState>
  ensureTab(relpath: string): void
  setLoading(relpath: string, loading: boolean): void
  setError(relpath: string, error: string | null): void
  setBase(relpath: string, base: Base): void
  setActiveView(relpath: string, name: string | null): void
  setSort(relpath: string, sort: SortState | null): void
  setSelectedRecordId(relpath: string, id: string | null): void
  setViewMode(relpath: string, mode: ViewMode): void
  setBoardGroupField(relpath: string, field: string | null): void
  setListGroupField(relpath: string, field: string | null): void
  toggleGroupCollapsed(relpath: string, key: string): void
  setCalendarDateField(relpath: string, field: string | null): void
  setCalendarMonth(relpath: string, yyyymm: string | null): void
  setGalleryImageField(relpath: string, field: string | null): void
  setTimelineGroupField(relpath: string, field: string | null): void
  setTimelineStartField(relpath: string, field: string | null): void
  setTimelineEndField(relpath: string, field: string | null): void
  setTimelineDayPx(relpath: string, px: number): void
  setSchemaEditorOpen(relpath: string, open: boolean): void
  /** Toggle the trash filter for the tab — see `BasesTabState.trashOpen`. */
  setTrashOpen(relpath: string, open: boolean): void
  /** Replace the hidden-fields list for the tab. `null` = show all. */
  setHiddenFields(relpath: string, fields: string[] | null): void
  /** Replace the view filter rules for the tab. */
  setViewFilters(relpath: string, filters: FilterRule[]): void
  /** Patch a single record's fields in the local cache. The caller
   *  is responsible for having already committed through the kernel;
   *  this just keeps the UI consistent without a full reload. */
  patchRecord(relpath: string, recordId: string, fields: Record<string, unknown>): void
  /** Append a freshly-created record (from `base_record_create`). */
  appendRecord(relpath: string, record: BaseRecord): void
  /** Drop a record from the local cache after `base_record_delete`. */
  removeRecord(relpath: string, recordId: string): void
  /** Replace the views array on the local base (after a `base_view_*`
   *  mutation has already been committed to the kernel). */
  setViews(relpath: string, views: BaseView[]): void
  /** Record a fresh edit that was just applied via `entry.forward`.
   *  Clears the redo stack and caps the undo stack at UNDO_HISTORY_CAP. */
  pushHistory(relpath: string, entry: HistoryEntry): void
  /** Pop the top undo entry and run its `inverse`. Returns true if an
   *  entry was available, false on empty stack. */
  undo(relpath: string): Promise<boolean>
  /** Pop the top redo entry and re-run its `forward`. */
  redo(relpath: string): Promise<boolean>
  closeTab(relpath: string): void
  /** Mark the tab as read-only and capture any unsupported filter
   *  expressions reported by the kernel. Called once per tab on
   *  successful `.base` load. */
  setReadOnly(relpath: string, readOnly: boolean, unsupportedFilters: string[]): void
  /** Set the most recent undo/redo error message (or clear it via
   *  `null`). UI surfaces this as a dismissable banner; the next
   *  `pushHistory` call clears it automatically. */
  setLastUndoError(relpath: string, error: string | null): void
}

const EMPTY: BasesTabState = {
  base: null,
  loading: false,
  error: null,
  activeView: null,
  sort: null,
  selectedRecordId: null,
  viewMode: 'table',
  boardGroupField: null,
  listGroupField: null,
  collapsedGroups: new Set(),
  calendarDateField: null,
  calendarMonth: null,
  galleryImageField: null,
  timelineGroupField: null,
  timelineStartField: null,
  timelineEndField: null,
  timelineDayPx: 24,
  schemaEditorOpen: false,
  trashOpen: false,
  hiddenFields: null,
  viewFilters: [],
  undoStack: [],
  redoStack: [],
  readOnly: false,
  unsupportedFilters: [],
  lastUndoError: null,
}

export const useBasesStore = create<BasesStore>((set) => ({
  tabs: {},
  ensureTab(relpath) {
    set((s) =>
      s.tabs[relpath] ? s : { tabs: { ...s.tabs, [relpath]: { ...EMPTY } } },
    )
  },
  setLoading(relpath, loading) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, loading } } }
    })
  },
  setError(relpath, error) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, error, loading: false } } }
    })
  },
  setBase(relpath, base) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return {
        tabs: {
          ...s.tabs,
          [relpath]: { ...t, base, loading: false, error: null },
        },
      }
    })
  },
  setActiveView(relpath, name) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, activeView: name } } }
    })
  },
  setSort(relpath, sort) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, sort } } }
    })
  },
  setSelectedRecordId(relpath, id) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, selectedRecordId: id } } }
    })
  },
  setViewMode(relpath, mode) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, viewMode: mode } } }
    })
  },
  setBoardGroupField(relpath, field) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, boardGroupField: field } } }
    })
  },
  setListGroupField(relpath, field) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, listGroupField: field } } }
    })
  },
  setCalendarDateField(relpath, field) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, calendarDateField: field } } }
    })
  },
  setCalendarMonth(relpath, yyyymm) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, calendarMonth: yyyymm } } }
    })
  },
  setGalleryImageField(relpath, field) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, galleryImageField: field } } }
    })
  },
  setTimelineGroupField(relpath, field) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, timelineGroupField: field } } }
    })
  },
  setTimelineStartField(relpath, field) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, timelineStartField: field } } }
    })
  },
  setTimelineEndField(relpath, field) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, timelineEndField: field } } }
    })
  },
  setTimelineDayPx(relpath, px) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      const clamped = Math.max(TIMELINE_DAY_PX_MIN, Math.min(TIMELINE_DAY_PX_MAX, px))
      return { tabs: { ...s.tabs, [relpath]: { ...t, timelineDayPx: clamped } } }
    })
  },
  setSchemaEditorOpen(relpath, open) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, schemaEditorOpen: open } } }
    })
  },
  setTrashOpen(relpath, open) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      // Reset selection when switching modes — selected ids are
      // disjoint between live and trash sets.
      return {
        tabs: {
          ...s.tabs,
          [relpath]: { ...t, trashOpen: open, selectedRecordId: null },
        },
      }
    })
  },
  setHiddenFields(relpath, fields) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, hiddenFields: fields } } }
    })
  },
  setViewFilters(relpath, filters) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return { tabs: { ...s.tabs, [relpath]: { ...t, viewFilters: filters } } }
    })
  },
  toggleGroupCollapsed(relpath, key) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      const next = new Set(t.collapsedGroups)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return { tabs: { ...s.tabs, [relpath]: { ...t, collapsedGroups: next } } }
    })
  },
  patchRecord(relpath, recordId, fields) {
    set((s) => {
      const t = s.tabs[relpath]
      if (!t?.base) return s
      const records = t.base.records.map((r): BaseRecord =>
        r.id === recordId ? { ...r, ...fields, id: r.id } : r,
      )
      return {
        tabs: { ...s.tabs, [relpath]: { ...t, base: { ...t.base, records } } },
      }
    })
  },
  appendRecord(relpath, record) {
    set((s) => {
      const t = s.tabs[relpath]
      if (!t?.base) return s
      const records = [...t.base.records, record]
      return {
        tabs: { ...s.tabs, [relpath]: { ...t, base: { ...t.base, records } } },
      }
    })
  },
  pushHistory(relpath, entry) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      const next = [...t.undoStack, entry]
      if (next.length > UNDO_HISTORY_CAP) next.splice(0, next.length - UNDO_HISTORY_CAP)
      return {
        tabs: {
          ...s.tabs,
          // A fresh edit clears any stale undo/redo error banner —
          // the user has moved on.
          [relpath]: { ...t, undoStack: next, redoStack: [], lastUndoError: null },
        },
      }
    })
  },
  async undo(relpath) {
    const t = useBasesStore.getState().tabs[relpath]
    if (!t || t.undoStack.length === 0) return false
    const entry = t.undoStack[t.undoStack.length - 1]
    try {
      await entry.inverse()
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      set((s) => {
        const cur = s.tabs[relpath]
        if (!cur) return s
        return {
          tabs: { ...s.tabs, [relpath]: { ...cur, lastUndoError: `undo failed: ${msg}` } },
        }
      })
      return false
    }
    set((s) => {
      const cur = s.tabs[relpath]
      if (!cur) return s
      return {
        tabs: {
          ...s.tabs,
          [relpath]: {
            ...cur,
            undoStack: cur.undoStack.slice(0, -1),
            redoStack: [...cur.redoStack, entry],
            lastUndoError: null,
          },
        },
      }
    })
    return true
  },
  async redo(relpath) {
    const t = useBasesStore.getState().tabs[relpath]
    if (!t || t.redoStack.length === 0) return false
    const entry = t.redoStack[t.redoStack.length - 1]
    try {
      await entry.forward()
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      set((s) => {
        const cur = s.tabs[relpath]
        if (!cur) return s
        return {
          tabs: { ...s.tabs, [relpath]: { ...cur, lastUndoError: `redo failed: ${msg}` } },
        }
      })
      return false
    }
    set((s) => {
      const cur = s.tabs[relpath]
      if (!cur) return s
      return {
        tabs: {
          ...s.tabs,
          [relpath]: {
            ...cur,
            redoStack: cur.redoStack.slice(0, -1),
            undoStack: [...cur.undoStack, entry],
            lastUndoError: null,
          },
        },
      }
    })
    return true
  },
  setViews(relpath, views) {
    set((s) => {
      const t = s.tabs[relpath]
      if (!t?.base) return s
      return {
        tabs: { ...s.tabs, [relpath]: { ...t, base: { ...t.base, views } } },
      }
    })
  },
  removeRecord(relpath, recordId) {
    set((s) => {
      const t = s.tabs[relpath]
      if (!t?.base) return s
      const records = t.base.records.filter((r) => r.id !== recordId)
      const selectedRecordId =
        t.selectedRecordId === recordId ? null : t.selectedRecordId
      return {
        tabs: {
          ...s.tabs,
          [relpath]: { ...t, base: { ...t.base, records }, selectedRecordId },
        },
      }
    })
  },
  closeTab(relpath) {
    set((s) => {
      if (!s.tabs[relpath]) return s
      const { [relpath]: _gone, ...rest } = s.tabs
      return { tabs: rest }
    })
  },
  setReadOnly(relpath, readOnly, unsupportedFilters) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return {
        tabs: {
          ...s.tabs,
          [relpath]: { ...t, readOnly, unsupportedFilters },
        },
      }
    })
  },
  setLastUndoError(relpath, error) {
    set((s) => {
      const t = s.tabs[relpath] ?? { ...EMPTY }
      return {
        tabs: { ...s.tabs, [relpath]: { ...t, lastUndoError: error } },
      }
    })
  },
}))
