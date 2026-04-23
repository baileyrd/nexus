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

  const [opError, setOpError] = useState<string | null>(null)

  if (!tab) return null

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
    setActiveView(relpath, view.name)
  }

  const handleSave = async () => {
    if (!isPersistableMode(tab.viewMode)) {
      setOpError(`The ${tab.viewMode} view isn't persistable yet — save from Table / Board / Calendar / Gallery.`)
      return
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
      const view = viewFromTabState(name, tab.viewMode, tab)
      await client.createView(relpath, view)
      setViews(relpath, [...base.views, view])
      setActiveView(relpath, name)
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
    // Kernel update keys by name, so rename = delete + create.
    try {
      setOpError(null)
      const renamed: BaseView = { ...view, name }
      await client.deleteView(relpath, view.name)
      await client.createView(relpath, renamed)
      setViews(
        relpath,
        base.views.map((v) => (v.name === view.name ? renamed : v)),
      )
      if (tab.activeView === view.name) setActiveView(relpath, name)
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
    } catch (err) {
      setOpError(`duplicate failed: ${errMsg(err)}`)
    }
  }

  const handleDelete = async (view: BaseView) => {
    if (!window.confirm(`Delete view "${view.name}"?`)) return
    try {
      setOpError(null)
      await client.deleteView(relpath, view.name)
      setViews(relpath, base.views.filter((v) => v.name !== view.name))
      if (tab.activeView === view.name) setActiveView(relpath, null)
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
            onApply={() => handleApply(v)}
            onRename={() => void handleRename(v)}
            onDuplicate={() => void handleDuplicate(v)}
            onDelete={() => void handleDelete(v)}
          />
        )
      })}
      <button
        type="button"
        onClick={() => void handleSave()}
        title="Save the current mode + sort/group/date as a named view"
        style={newViewBtnStyle}
      >
        + New view
      </button>
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
  onApply(): void
  onRename(): void
  onDuplicate(): void
  onDelete(): void
}

function ViewPill({
  view,
  active,
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
      <button
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
      </button>
      {menuOpen && (
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
