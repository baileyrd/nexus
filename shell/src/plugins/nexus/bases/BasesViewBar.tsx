// Phase 5 of docs/bases-shell-plan.md — named-view pill bar. Lists
// `base.views` from the `.bases` TOML + wires save / rename /
// duplicate / delete through `base_view_*`. The kernel is the
// source of truth; local state is patched on success so the bar
// stays responsive without a full reload.

import { useState } from 'react'
import { useBasesStore } from './basesStore'
import type { Base, BasesKernelClient, BaseView } from './kernelClient'
import {
  applyView,
  filtersFromView,
  hiddenFieldsFromView,
  isPersistableMode,
  viewFromTabState,
} from './viewMapping'

interface Props {
  relpath: string
  base: Base
  client: BasesKernelClient
}

export function BasesViewBar({ relpath, base, client }: Props) {
  const tab = useBasesStore((s) => s.tabs[relpath])
  const setActiveView = useBasesStore((s) => s.setActiveView)
  const setViews = useBasesStore((s) => s.setViews)
  const setViewMode = useBasesStore((s) => s.setViewMode)
  const setSort = useBasesStore((s) => s.setSort)
  const setBoardGroupField = useBasesStore((s) => s.setBoardGroupField)
  const setCalendarDateField = useBasesStore((s) => s.setCalendarDateField)
  const setHiddenFields = useBasesStore((s) => s.setHiddenFields)
  const setViewFilters = useBasesStore((s) => s.setViewFilters)
  const pushHistory = useBasesStore((s) => s.pushHistory)

  const [opError, setOpError] = useState<string | null>(null)

  if (!tab) return null

  // Schema field list excluding the synthetic `id` column. Used to
  // derive the saved `view.fields` allowlist from the tab's
  // `hiddenFields` set and to invert that mapping on apply.
  const allFields = Object.keys(base.schema.fields ?? {}).filter(
    (f) => f !== 'id',
  )

  const handleApply = (view: BaseView) => {
    const applied = applyView(view)
    setViewMode(relpath, applied.mode)
    setSort(relpath, applied.sort)
    if (applied.mode === 'board') {
      setBoardGroupField(relpath, applied.boardGroupField)
    }
    if (applied.mode === 'calendar') {
      setCalendarDateField(relpath, applied.calendarDateField)
    }
    // Phase 5 round-trip — rehydrate hidden columns + filters from
    // the saved `view.fields` allowlist + `view.filter` rules.
    setHiddenFields(relpath, hiddenFieldsFromView(view, allFields))
    setViewFilters(relpath, filtersFromView(view))
    setActiveView(relpath, view.name)
  }

  /** Build the wire view from the current tab state. Includes the
   *  Phase-5 round-trip fields (`fields`, `filter`). */
  const snapshot = (name: string): BaseView =>
    viewFromTabState(name, tab.viewMode, tab, allFields)

  const handleSave = async () => {
    if (!isPersistableMode(tab.viewMode)) {
      setOpError(`The ${tab.viewMode} view isn't persistable yet — save from Table / Board / Calendar / Gallery.`)
      return
    }
    // If a named view is currently active, "Save" updates it in
    // place via `base_view_update` — which previously was dead code.
    // The active view name is the one the user expects "Save" to
    // overwrite; "Save as…" forks a new copy.
    if (tab.activeView) {
      const existing = base.views.find((v) => v.name === tab.activeView)
      if (existing) {
        try {
          setOpError(null)
          const prev: BaseView = JSON.parse(JSON.stringify(existing))
          const updated = snapshot(existing.name)
          await client.updateView(relpath, updated)
          setViews(
            relpath,
            base.views.map((v) => (v.name === existing.name ? updated : v)),
          )
          pushHistory(relpath, {
            label: `Save view "${existing.name}"`,
            forward: async () => {
              await client.updateView(relpath, updated)
              setViews(
                relpath,
                useBasesStore
                  .getState()
                  .tabs[relpath]?.base?.views.map((v) =>
                    v.name === existing.name ? updated : v,
                  ) ?? [updated],
              )
            },
            inverse: async () => {
              await client.updateView(relpath, prev)
              setViews(
                relpath,
                useBasesStore
                  .getState()
                  .tabs[relpath]?.base?.views.map((v) =>
                    v.name === existing.name ? prev : v,
                  ) ?? [prev],
              )
            },
          })
          return
        } catch (err) {
          setOpError(`save failed: ${errMsg(err)}`)
          return
        }
      }
    }
    const raw = window.prompt('Name this view', defaultName(tab.viewMode, base.views))
    if (!raw) return
    const name = raw.trim()
    if (!name) return
    if (base.views.some((v) => v.name === name)) {
      setOpError(`A view named "${name}" already exists.`)
      return
    }
    try {
      setOpError(null)
      const view = snapshot(name)
      await client.createView(relpath, view)
      setViews(relpath, [...base.views, view])
      setActiveView(relpath, name)
      pushHistory(relpath, {
        label: `Create view "${name}"`,
        forward: async () => {
          await client.createView(relpath, view)
          const cur = useBasesStore.getState().tabs[relpath]?.base?.views ?? []
          setViews(relpath, [...cur.filter((v) => v.name !== name), view])
        },
        inverse: async () => {
          await client.deleteView(relpath, name)
          const cur = useBasesStore.getState().tabs[relpath]?.base?.views ?? []
          setViews(relpath, cur.filter((v) => v.name !== name))
        },
      })
    } catch (err) {
      setOpError(`save failed: ${errMsg(err)}`)
    }
  }

  const handleRename = async (view: BaseView) => {
    const raw = window.prompt('Rename view', view.name)
    if (!raw) return
    const name = raw.trim()
    if (!name || name === view.name) return
    if (base.views.some((v) => v.name === name)) {
      setOpError(`A view named "${name}" already exists.`)
      return
    }
    // Kernel update keys by name, so rename = delete + create. We
    // snapshot the prior view for the inverse so undo restores both
    // the name and the wire fields exactly.
    try {
      setOpError(null)
      const prev: BaseView = JSON.parse(JSON.stringify(view))
      const renamed: BaseView = { ...view, name }
      await client.deleteView(relpath, view.name)
      await client.createView(relpath, renamed)
      setViews(
        relpath,
        base.views.map((v) => (v.name === view.name ? renamed : v)),
      )
      if (tab.activeView === view.name) setActiveView(relpath, name)
      pushHistory(relpath, {
        label: `Rename view ${view.name} → ${name}`,
        forward: async () => {
          await client.deleteView(relpath, prev.name)
          await client.createView(relpath, renamed)
          const cur = useBasesStore.getState().tabs[relpath]?.base?.views ?? []
          setViews(
            relpath,
            cur.map((v) => (v.name === prev.name ? renamed : v)),
          )
        },
        inverse: async () => {
          await client.deleteView(relpath, renamed.name)
          await client.createView(relpath, prev)
          const cur = useBasesStore.getState().tabs[relpath]?.base?.views ?? []
          setViews(
            relpath,
            cur.map((v) => (v.name === renamed.name ? prev : v)),
          )
        },
      })
    } catch (err) {
      setOpError(`rename failed: ${errMsg(err)}`)
    }
  }

  const handleDuplicate = async (view: BaseView) => {
    const name = nextCopyName(view.name, base.views)
    try {
      setOpError(null)
      const copy: BaseView = { ...view, name }
      await client.createView(relpath, copy)
      setViews(relpath, [...base.views, copy])
      pushHistory(relpath, {
        label: `Duplicate view ${view.name}`,
        forward: async () => {
          await client.createView(relpath, copy)
          const cur = useBasesStore.getState().tabs[relpath]?.base?.views ?? []
          setViews(relpath, [...cur.filter((v) => v.name !== name), copy])
        },
        inverse: async () => {
          await client.deleteView(relpath, name)
          const cur = useBasesStore.getState().tabs[relpath]?.base?.views ?? []
          setViews(relpath, cur.filter((v) => v.name !== name))
        },
      })
    } catch (err) {
      setOpError(`duplicate failed: ${errMsg(err)}`)
    }
  }

  const handleDelete = async (view: BaseView) => {
    if (!window.confirm(`Delete view "${view.name}"?`)) return
    try {
      setOpError(null)
      const prev: BaseView = JSON.parse(JSON.stringify(view))
      await client.deleteView(relpath, view.name)
      setViews(relpath, base.views.filter((v) => v.name !== view.name))
      if (tab.activeView === view.name) setActiveView(relpath, null)
      pushHistory(relpath, {
        label: `Delete view "${view.name}"`,
        forward: async () => {
          await client.deleteView(relpath, prev.name)
          const cur = useBasesStore.getState().tabs[relpath]?.base?.views ?? []
          setViews(relpath, cur.filter((v) => v.name !== prev.name))
        },
        inverse: async () => {
          await client.createView(relpath, prev)
          const cur = useBasesStore.getState().tabs[relpath]?.base?.views ?? []
          setViews(relpath, [...cur.filter((v) => v.name !== prev.name), prev])
        },
      })
    } catch (err) {
      setOpError(`delete failed: ${errMsg(err)}`)
    }
  }

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        flexWrap: 'wrap',
        gap: 4,
        padding: '4px 12px',
        borderBottom: '1px solid var(--border-subtle, #2a2a2e)',
        background: 'var(--bg-raised-dim, #1a1a1d)',
        fontSize: 11,
      }}
    >
      {base.views.map((v) => {
        const active = tab.activeView === v.name
        return (
          <ViewPill
            key={v.name}
            view={v}
            active={active}
            readOnly={tab.readOnly}
            onApply={() => handleApply(v)}
            onRename={() => void handleRename(v)}
            onDuplicate={() => void handleDuplicate(v)}
            onDelete={() => void handleDelete(v)}
          />
        )
      })}
      {!tab.readOnly && <button
        type="button"
        onClick={() => void handleSave()}
        title={
          tab.activeView && base.views.some((v) => v.name === tab.activeView)
            ? `Save changes to "${tab.activeView}" (sort / group / hidden columns / filters)`
            : 'Save the current mode + sort/group/date/hidden-columns/filters as a named view'
        }
        style={newViewBtnStyle}
      >
        {tab.activeView && base.views.some((v) => v.name === tab.activeView)
          ? `Save "${tab.activeView}"`
          : '+ New view'}
      </button>}
      {opError && (
        <span
          style={{
            color: 'var(--risk, #f48771)',
            marginLeft: 8,
          }}
        >
          {opError}
        </span>
      )}
    </div>
  )
}

interface PillProps {
  view: BaseView
  active: boolean
  readOnly: boolean
  onApply(): void
  onRename(): void
  onDuplicate(): void
  onDelete(): void
}

function ViewPill({
  view,
  active,
  readOnly,
  onApply,
  onRename,
  onDuplicate,
  onDelete,
}: PillProps) {
  const [menuOpen, setMenuOpen] = useState(false)
  return (
    <span style={{ position: 'relative', display: 'inline-flex' }}>
      <button
        type="button"
        onClick={onApply}
        title={`Apply view · type=${view.type}`}
        style={{
          padding: '2px 8px',
          background: active ? 'var(--accent, #60a5fa)' : 'var(--bg-raised, #252529)',
          color: active ? 'var(--fg-on-accent, #0f1117)' : 'var(--fg-primary, #e4e4e7)',
          border: '1px solid var(--border-subtle, #2a2a2e)',
          borderTopRightRadius: 0,
          borderBottomRightRadius: 0,
          borderRight: 'none',
          borderRadius: '3px 0 0 3px',
          cursor: 'pointer',
        }}
      >
        {view.name}
      </button>
      {!readOnly && <button
        type="button"
        onClick={() => setMenuOpen((v) => !v)}
        title="Rename / Duplicate / Delete"
        style={{
          padding: '2px 6px',
          background: active ? 'var(--accent, #60a5fa)' : 'var(--bg-raised, #252529)',
          color: active ? 'var(--fg-on-accent, #0f1117)' : 'var(--fg-muted, #9ca3af)',
          border: '1px solid var(--border-subtle, #2a2a2e)',
          borderRadius: '0 3px 3px 0',
          cursor: 'pointer',
        }}
      >
        ⋯
      </button>}
      {!readOnly && menuOpen && (
        <>
          <div
            onClick={() => setMenuOpen(false)}
            style={{
              position: 'fixed',
              inset: 0,
              zIndex: 10,
            }}
          />
          <div
            style={{
              position: 'absolute',
              top: '100%',
              right: 0,
              zIndex: 11,
              background: 'var(--bg-raised, #252529)',
              border: '1px solid var(--border-subtle, #2a2a2e)',
              borderRadius: 4,
              boxShadow: '0 4px 12px rgba(0,0,0,0.3)',
              display: 'flex',
              flexDirection: 'column',
              minWidth: 140,
              padding: 4,
              marginTop: 2,
            }}
          >
            <MenuItem
              label="Rename"
              onClick={() => {
                setMenuOpen(false)
                onRename()
              }}
            />
            <MenuItem
              label="Duplicate"
              onClick={() => {
                setMenuOpen(false)
                onDuplicate()
              }}
            />
            <MenuItem
              label="Delete"
              destructive
              onClick={() => {
                setMenuOpen(false)
                onDelete()
              }}
            />
          </div>
        </>
      )}
    </span>
  )
}

function MenuItem({
  label,
  onClick,
  destructive,
}: {
  label: string
  onClick(): void
  destructive?: boolean
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        padding: '4px 10px',
        background: 'transparent',
        border: 'none',
        color: destructive ? 'var(--risk, #f48771)' : 'var(--fg-primary, #e4e4e7)',
        textAlign: 'left',
        fontSize: 11,
        cursor: 'pointer',
        borderRadius: 3,
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = 'var(--bg-hover, #303036)'
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = 'transparent'
      }}
    >
      {label}
    </button>
  )
}

function defaultName(mode: string, existing: BaseView[]): string {
  const base = `${mode.charAt(0).toUpperCase()}${mode.slice(1)} view`
  if (!existing.some((v) => v.name === base)) return base
  for (let i = 2; i < 100; i += 1) {
    const candidate = `${base} ${i}`
    if (!existing.some((v) => v.name === candidate)) return candidate
  }
  return `${base} ${Date.now()}`
}

function nextCopyName(name: string, existing: BaseView[]): string {
  for (let i = 1; i < 100; i += 1) {
    const candidate = i === 1 ? `${name} copy` : `${name} copy ${i}`
    if (!existing.some((v) => v.name === candidate)) return candidate
  }
  return `${name} copy ${Date.now()}`
}

function errMsg(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

const newViewBtnStyle: React.CSSProperties = {
  padding: '2px 8px',
  background: 'transparent',
  color: 'var(--fg-muted, #9ca3af)',
  border: '1px dashed var(--border-subtle, #2a2a2e)',
  borderRadius: 3,
  fontSize: 11,
  cursor: 'pointer',
  marginLeft: 4,
}
