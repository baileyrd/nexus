// Phase 2 of docs/bases-shell-plan.md — Table view. Rows are
// windowed via @tanstack/react-virtual so a 50k-row base still
// scrolls smoothly; header is sticky and row heights are fixed so
// we can use a simple index→translateY layout.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useBasesStore, type SortDir } from './basesStore'
import type { Base, BaseRecord, BasesKernelClient } from './kernelClient'
import {
  defaultValueFor,
  formatValue,
  isReadOnly,
  parseFieldDef,
  typeGlyph,
  type FieldDefinition,
  type FieldKind,
} from './fieldTypes'
import { getBasesApi } from './runtime'

interface Props {
  relpath: string
  base: Base
  client: BasesKernelClient
}

interface Column {
  name: string
  def: FieldDefinition
}

export function BasesTable({ relpath, base, client }: Props) {
  const sort = useBasesStore((s) => s.tabs[relpath]?.sort ?? null)
  const selectedRecordId = useBasesStore(
    (s) => s.tabs[relpath]?.selectedRecordId ?? null,
  )
  const undoLen = useBasesStore((s) => s.tabs[relpath]?.undoStack.length ?? 0)
  const redoLen = useBasesStore((s) => s.tabs[relpath]?.redoStack.length ?? 0)
  const trashOpen = useBasesStore((s) => s.tabs[relpath]?.trashOpen ?? false)
  const readOnly = useBasesStore((s) => s.tabs[relpath]?.readOnly ?? false)
  const setSort = useBasesStore((s) => s.setSort)
  const setSelectedRecordId = useBasesStore((s) => s.setSelectedRecordId)
  const patchRecord = useBasesStore((s) => s.patchRecord)
  const appendRecord = useBasesStore((s) => s.appendRecord)
  const removeRecord = useBasesStore((s) => s.removeRecord)
  const pushHistory = useBasesStore((s) => s.pushHistory)
  const undo = useBasesStore((s) => s.undo)
  const redo = useBasesStore((s) => s.redo)

  const [editing, setEditing] = useState<{ id: string; field: string } | null>(null)
  const [opError, setOpError] = useState<string | null>(null)

  const columns = useMemo<Column[]>(() => buildColumns(base), [base])
  const records = useMemo(
    () => sortRecords(base.records, sort, columns),
    [base.records, sort, columns],
  )

  const handleHeaderClick = (name: string) => {
    if (!sort || sort.field !== name) {
      setSort(relpath, { field: name, dir: 'asc' })
      return
    }
    if (sort.dir === 'asc') {
      setSort(relpath, { field: name, dir: 'desc' })
    } else {
      setSort(relpath, null)
    }
  }

  const commitEdit = useCallback(
    async (recordId: string, field: string, value: unknown) => {
      // Capture the pre-edit value so the inverse writes the record
      // back to exactly what the kernel had before. Read off the
      // latest store state to beat races with the local patch.
      const prev = useBasesStore.getState().tabs[relpath]?.base?.records.find((r) => r.id === recordId)?.[field]
      try {
        setOpError(null)
        await client.updateRecord(relpath, recordId, { [field]: value })
        patchRecord(relpath, recordId, { [field]: value })
        pushHistory(relpath, {
          label: `Edit ${field}`,
          forward: async () => {
            await client.updateRecord(relpath, recordId, { [field]: value })
            patchRecord(relpath, recordId, { [field]: value })
          },
          inverse: async () => {
            await client.updateRecord(relpath, recordId, { [field]: prev })
            patchRecord(relpath, recordId, { [field]: prev })
          },
        })
      } catch (err) {
        setOpError(`update failed: ${errMsg(err)}`)
      } finally {
        setEditing(null)
      }
    },
    [client, relpath, patchRecord, pushHistory],
  )

  const handleAddRow = async () => {
    try {
      setOpError(null)
      const seed: Record<string, unknown> = {}
      for (const { name, def } of columns) {
        if (def.required && !isReadOnly(def.type)) {
          seed[name] = defaultValueFor(def.type)
        }
      }
      const stored = await client.createRecord(relpath, {
        id: '',
        ...seed,
      } as BaseRecord)
      appendRecord(relpath, stored)
      setSelectedRecordId(relpath, stored.id)
      pushHistory(relpath, {
        label: 'Add row',
        // Redo re-creates with the same id so subsequent history
        // entries targeting `stored.id` stay valid.
        forward: async () => {
          await client.createRecord(relpath, stored)
          appendRecord(relpath, stored)
        },
        inverse: async () => {
          await client.deleteRecord(relpath, stored.id)
          removeRecord(relpath, stored.id)
        },
      })
    } catch (err) {
      setOpError(`create failed: ${errMsg(err)}`)
    }
  }

  /** Default delete action (live view). Soft-deletes the record via
   *  `base_record_soft_delete` so it lands in the trash view and can
   *  be restored — WI-10 §4.2 acceptance. The kernel stamps
   *  `deleted_at`; we mirror that locally via `patchRecord` so
   *  `BasesView.filter((r) => !r.deletedAt)` hides it on next render.
   *  Undo flips back through `base_record_restore`. */
  const handleSoftDeleteRow = useCallback(
    async (recordId: string) => {
      const api = getBasesApi()
      // `api.input.confirm` on native routes through the OS dialog;
      // in tests/headless it isn't installed, so we soft-delete
      // without prompting if the runtime isn't wired yet.
      if (api) {
        const ok = await api.input.confirm('Move this record to the trash?')
        if (!ok) return
      }
      try {
        setOpError(null)
        await client.softDeleteRecord(relpath, recordId)
        const stamp = Math.floor(Date.now() / 1000)
        patchRecord(relpath, recordId, { deletedAt: stamp })
        pushHistory(relpath, {
          label: 'Soft-delete row',
          forward: async () => {
            await client.softDeleteRecord(relpath, recordId)
            patchRecord(relpath, recordId, { deletedAt: Math.floor(Date.now() / 1000) })
          },
          inverse: async () => {
            await client.restoreRecord(relpath, recordId)
            patchRecord(relpath, recordId, { deletedAt: null })
          },
        })
      } catch (err) {
        setOpError(`soft-delete failed: ${errMsg(err)}`)
      }
    },
    [client, relpath, patchRecord, pushHistory],
  )

  /** Restore a soft-deleted record — only reachable from the trash
   *  view. Mirror of `handleSoftDeleteRow`. */
  const handleRestoreRow = useCallback(
    async (recordId: string) => {
      try {
        setOpError(null)
        await client.restoreRecord(relpath, recordId)
        patchRecord(relpath, recordId, { deletedAt: null })
        pushHistory(relpath, {
          label: 'Restore row',
          forward: async () => {
            await client.restoreRecord(relpath, recordId)
            patchRecord(relpath, recordId, { deletedAt: null })
          },
          inverse: async () => {
            await client.softDeleteRecord(relpath, recordId)
            patchRecord(relpath, recordId, { deletedAt: Math.floor(Date.now() / 1000) })
          },
        })
      } catch (err) {
        setOpError(`restore failed: ${errMsg(err)}`)
      }
    },
    [client, relpath, patchRecord, pushHistory],
  )

  /** Permanent hard-delete. Only reachable from the trash view so the
   *  user doesn't lose data from a reflex Backspace on a live record.
   *  Prompts via `api.input.confirm` and skips the undo stack — the
   *  kernel can't resurrect a hard-deleted record. */
  const handleHardDeleteRow = useCallback(
    async (recordId: string) => {
      const api = getBasesApi()
      if (api) {
        const ok = await api.input.confirm(
          'Delete this record forever? This cannot be undone.',
        )
        if (!ok) return
      }
      try {
        setOpError(null)
        await client.deleteRecord(relpath, recordId)
        removeRecord(relpath, recordId)
      } catch (err) {
        setOpError(`delete failed: ${errMsg(err)}`)
      }
    },
    [client, relpath, removeRecord],
  )

  // Single handler the toolbar / Backspace path dispatches to based
  // on whether the user is looking at the trash. In trash mode
  // Backspace still does the hard-delete (user is already in a
  // destructive-actions surface), matching the "Delete forever"
  // button next to each trashed row.
  const handleDeleteRow = useCallback(
    async (recordId: string) => {
      if (trashOpen) {
        await handleHardDeleteRow(recordId)
      } else {
        await handleSoftDeleteRow(recordId)
      }
    },
    [trashOpen, handleHardDeleteRow, handleSoftDeleteRow],
  )

  const handleExportCsv = async () => {
    try {
      setOpError(null)
      const fieldNames = columns.map((c) => c.name)
      const bytes = await client.csvExport(base.records, fieldNames)
      // Cast: zustand's DOM lib.d.ts narrows BlobPart to ArrayBuffer-backed
      // views, but our Uint8Array's `buffer` is an `ArrayBufferLike` so the
      // structural check fails. The value is safe at runtime.
      const blob = new Blob([bytes as BlobPart], { type: 'text/csv;charset=utf-8' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      const stem = filenameStem(relpath)
      a.href = url
      a.download = `${stem || 'base'}.csv`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)
    } catch (err) {
      setOpError(`export failed: ${errMsg(err)}`)
    }
  }

  const fileInputRef = useRef<HTMLInputElement>(null)
  const handleImportCsv = () => fileInputRef.current?.click()
  const handleImportFile = async (file: File) => {
    try {
      setOpError(null)
      const buf = new Uint8Array(await file.arrayBuffer())
      const fieldNames = columns.map((c) => c.name)
      const result = await client.csvImport(buf, fieldNames, true)
      let imported = 0
      const created: BaseRecord[] = []
      for (const r of result.records) {
        try {
          const stored = await client.createRecord(relpath, r)
          appendRecord(relpath, stored)
          created.push(stored)
          imported += 1
        } catch (err) {
          result.errors.push([imported, errMsg(err)])
        }
      }
      if (created.length > 0) {
        pushHistory(relpath, {
          label: `Import ${created.length} rows`,
          forward: async () => {
            for (const r of created) {
              await client.createRecord(relpath, r)
              appendRecord(relpath, r)
            }
          },
          inverse: async () => {
            for (const r of created) {
              await client.deleteRecord(relpath, r.id)
              removeRecord(relpath, r.id)
            }
          },
        })
      }
      const msg = `Imported ${imported}, skipped ${result.skipped}${
        result.errors.length ? `, ${result.errors.length} errors` : ''
      }`
      if (result.errors.length) {
        setOpError(msg)
      } else {
        setOpError(null)
        // Surface success briefly via the error channel styled neutrally.
        setOpError(msg)
      }
    } catch (err) {
      setOpError(`import failed: ${errMsg(err)}`)
    }
  }

  // Keyboard: Backspace / Delete on the table body removes the
  // selected row (when no cell is being edited). Arrow keys nav
  // rows. Bind on the outer container, gated by `editing == null`.
  // Undo/redo flow through the global KeybindingRegistry — see
  // BasesView.tsx focusin handler + activeBases.ts.
  const containerRef = useRef<HTMLDivElement>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const ROW_HEIGHT = 28
  const rowVirtualizer = useVirtualizer({
    count: records.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
  })
  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const onKey = (e: KeyboardEvent) => {
      if (editing) return
      // BL-030: Mod-Z / Mod-Shift-Z / Mod-Y are now handled by the
      // global KeybindingRegistry, gated on `bases.focused`. This
      // local handler keeps Backspace/Delete + arrow-key navigation,
      // which are scoped to the table view's selection model and
      // shouldn't fire from the schema editor or view bar.
      if (!selectedRecordId) return
      if (e.key === 'Delete' || e.key === 'Backspace') {
        e.preventDefault()
        void handleDeleteRow(selectedRecordId)
        return
      }
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        const i = records.findIndex((r) => r.id === selectedRecordId)
        if (i < 0) return
        const next =
          e.key === 'ArrowDown'
            ? records[Math.min(i + 1, records.length - 1)]
            : records[Math.max(i - 1, 0)]
        if (next) {
          setSelectedRecordId(relpath, next.id)
          e.preventDefault()
        }
      }
    }
    el.addEventListener('keydown', onKey)
    return () => el.removeEventListener('keydown', onKey)
  }, [editing, selectedRecordId, records, relpath, setSelectedRecordId, handleDeleteRow])

  return (
    <div
      ref={containerRef}
      tabIndex={0}
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        outline: 'none',
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '6px 12px',
          borderBottom: '1px solid var(--border-subtle, #2a2a2e)',
          fontSize: 12,
          color: 'var(--fg-muted, #9ca3af)',
        }}
      >
        {!readOnly && <button
          type="button"
          onClick={() => void handleAddRow()}
          disabled={trashOpen}
          title={trashOpen ? 'Cannot add rows while viewing the trash' : undefined}
          style={{ ...toolbarBtnStyle, opacity: trashOpen ? 0.4 : 1 }}
        >
          + New row
        </button>}
        {!readOnly && selectedRecordId && !trashOpen && (
          <button
            type="button"
            onClick={() => void handleSoftDeleteRow(selectedRecordId)}
            title="Move to trash"
            style={toolbarBtnStyle}
          >
            Move to trash
          </button>
        )}
        {!readOnly && selectedRecordId && trashOpen && (
          <>
            <button
              type="button"
              onClick={() => void handleRestoreRow(selectedRecordId)}
              title="Restore from trash"
              style={toolbarBtnStyle}
            >
              Restore
            </button>
            <button
              type="button"
              onClick={() => void handleHardDeleteRow(selectedRecordId)}
              title="Permanently delete (cannot be undone)"
              style={{ ...toolbarBtnStyle, color: 'var(--risk, #f48771)' }}
            >
              Delete forever
            </button>
          </>
        )}
        {!readOnly && <button
          type="button"
          disabled={undoLen === 0}
          onClick={() => void undo(relpath)}
          title="Undo (Ctrl/Cmd+Z)"
          style={{ ...toolbarBtnStyle, opacity: undoLen === 0 ? 0.4 : 1 }}
        >
          Undo{undoLen > 0 ? ` (${undoLen})` : ''}
        </button>}
        {!readOnly && <button
          type="button"
          disabled={redoLen === 0}
          onClick={() => void redo(relpath)}
          title="Redo (Ctrl/Cmd+Shift+Z)"
          style={{ ...toolbarBtnStyle, opacity: redoLen === 0 ? 0.4 : 1 }}
        >
          Redo{redoLen > 0 ? ` (${redoLen})` : ''}
        </button>}
        {!readOnly && <button type="button" onClick={handleImportCsv} style={toolbarBtnStyle}>
          Import CSV
        </button>}
        <button type="button" onClick={() => void handleExportCsv()} style={toolbarBtnStyle}>
          Export CSV
        </button>
        <input
          ref={fileInputRef}
          type="file"
          accept=".csv,text/csv"
          style={{ display: 'none' }}
          onChange={(e) => {
            const file = e.target.files?.[0]
            if (file) void handleImportFile(file)
            e.target.value = ''
          }}
        />
        {sort && (
          <button type="button" onClick={() => setSort(relpath, null)} style={toolbarBtnStyle}>
            Clear sort ({sort.field} {sort.dir})
          </button>
        )}
        {opError && <span style={{ color: 'var(--risk, #f48771)' }}>{opError}</span>}
      </div>
      <div ref={scrollRef} style={{ flex: 1, overflow: 'auto' }}>
        <table
          style={{
            borderCollapse: 'collapse',
            width: '100%',
            fontSize: 12,
            tableLayout: 'fixed',
          }}
        >
          <colgroup>
            {columns.map((c) => (
              <col
                key={c.name}
                style={{
                  width: c.def.type === 'long-text' ? 320 : c.def.type === 'checkbox' ? 56 : 180,
                }}
              />
            ))}
          </colgroup>
          <thead>
            <tr>
              {columns.map((c) => {
                const active = sort?.field === c.name
                const arrow = active ? (sort.dir === 'asc' ? '▲' : '▼') : ''
                return (
                  <th
                    key={c.name}
                    onClick={() => handleHeaderClick(c.name)}
                    style={{
                      padding: '6px 10px',
                      textAlign: 'left',
                      background: 'var(--bg-raised, #252529)',
                      borderBottom: '1px solid var(--border-subtle, #2a2a2e)',
                      borderRight: '1px solid var(--border-subtle, #2a2a2e)',
                      color: 'var(--fg-muted, #9ca3af)',
                      fontWeight: 500,
                      cursor: 'pointer',
                      userSelect: 'none',
                      position: 'sticky',
                      top: 0,
                      zIndex: 1,
                    }}
                    title={`${c.def.type}${c.def.primary ? ' · primary' : ''}${c.def.required ? ' · required' : ''}`}
                  >
                    <span
                      aria-hidden
                      style={{
                        display: 'inline-block',
                        width: 14,
                        color: 'var(--fg-dim, #6b7280)',
                      }}
                    >
                      {typeGlyph(c.def.type)}
                    </span>
                    <span style={{ color: 'var(--fg-primary, #e4e4e7)' }}>{c.def.displayName ?? c.name}</span>
                    {arrow && (
                      <span style={{ marginLeft: 6, color: 'var(--accent, #60a5fa)' }}>
                        {arrow}
                      </span>
                    )}
                  </th>
                )
              })}
            </tr>
          </thead>
          <tbody>
            {(() => {
              if (records.length === 0) return null
              const virtualRows = rowVirtualizer.getVirtualItems()
              const total = rowVirtualizer.getTotalSize()
              const topPad = virtualRows.length > 0 ? virtualRows[0].start : 0
              const bottomPad =
                virtualRows.length > 0
                  ? total - virtualRows[virtualRows.length - 1].end
                  : 0
              return (
                <>
                  {topPad > 0 && (
                    <tr style={{ height: topPad }}>
                      <td colSpan={columns.length} style={{ padding: 0, border: 0 }} />
                    </tr>
                  )}
                  {virtualRows.map((vr) => {
                    const r = records[vr.index]
                    if (!r) return null
                    return (
                      <Row
                        key={r.id}
                        record={r}
                        columns={columns}
                        selected={r.id === selectedRecordId}
                        editing={editing?.id === r.id ? editing.field : null}
                        client={client}
                        rowHeight={ROW_HEIGHT}
                        onSelect={() => setSelectedRecordId(relpath, r.id)}
                        onStartEdit={(field) => {
                          setSelectedRecordId(relpath, r.id)
                          if (!readOnly) setEditing({ id: r.id, field })
                        }}
                        onCancelEdit={() => setEditing(null)}
                        onCommit={(field, value) => void commitEdit(r.id, field, value)}
                      />
                    )
                  })}
                  {bottomPad > 0 && (
                    <tr style={{ height: bottomPad }}>
                      <td colSpan={columns.length} style={{ padding: 0, border: 0 }} />
                    </tr>
                  )}
                </>
              )
            })()}
            {records.length === 0 && (
              <tr>
                <td
                  colSpan={columns.length}
                  style={{
                    padding: 24,
                    textAlign: 'center',
                    color: 'var(--fg-muted, #9ca3af)',
                  }}
                >
                  No records. Use "+ New row" to add one.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  )
}

interface RowProps {
  record: BaseRecord
  columns: Column[]
  selected: boolean
  editing: string | null
  client: BasesKernelClient
  rowHeight: number
  onSelect(): void
  onStartEdit(field: string): void
  onCancelEdit(): void
  onCommit(field: string, value: unknown): void
}

function Row({
  record,
  columns,
  selected,
  editing,
  client,
  rowHeight,
  onSelect,
  onStartEdit,
  onCancelEdit,
  onCommit,
}: RowProps) {
  return (
    <tr
      onClick={onSelect}
      style={{
        background: selected ? 'var(--bg-selection, #2a2a35)' : 'transparent',
        cursor: 'default',
        height: rowHeight,
      }}
    >
      {columns.map((c) => (
        <Cell
          key={c.name}
          field={c.name}
          def={c.def}
          value={record[c.name]}
          record={record}
          client={client}
          editing={editing === c.name}
          onStartEdit={() => onStartEdit(c.name)}
          onCancel={onCancelEdit}
          onCommit={(v) => onCommit(c.name, v)}
        />
      ))}
    </tr>
  )
}

interface CellProps {
  field: string
  def: FieldDefinition
  value: unknown
  record: BaseRecord
  client: BasesKernelClient
  editing: boolean
  onStartEdit(): void
  onCancel(): void
  onCommit(value: unknown): void
}

function Cell({ field, def, value, record, client, editing, onStartEdit, onCancel, onCommit }: CellProps) {
  const readOnly = isReadOnly(def.type)
  const base: React.CSSProperties = {
    padding: '4px 10px',
    borderBottom: '1px solid var(--border-subtle, #2a2a2e)',
    borderRight: '1px solid var(--border-subtle, #2a2a2e)',
    verticalAlign: 'middle',
    color: readOnly ? 'var(--fg-muted, #9ca3af)' : 'var(--fg-primary, #e4e4e7)',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  }

  // Checkbox toggles without an edit mode.
  if (def.type === 'checkbox' && !readOnly) {
    const checked = value === true
    return (
      <td style={base}>
        <input
          type="checkbox"
          checked={checked}
          onChange={(e) => onCommit(e.currentTarget.checked)}
          onClick={(e) => e.stopPropagation()}
        />
      </td>
    )
  }

  if (editing && !readOnly) {
    return (
      <td style={{ ...base, padding: 0 }} onClick={(e) => e.stopPropagation()}>
        <CellEditor def={def} value={value} onCommit={onCommit} onCancel={onCancel} />
      </td>
    )
  }

  if (def.type === 'formula' && def.expression) {
    return (
      <td style={base} title={`formula · ${def.expression}`}>
        <FormulaCell
          expression={def.expression}
          record={record}
          client={client}
        />
      </td>
    )
  }

  return (
    <td
      style={base}
      onDoubleClick={(e) => {
        e.stopPropagation()
        if (!readOnly) onStartEdit()
      }}
      title={readOnly ? `${def.type} (read-only)` : undefined}
    >
      {renderReadCell(def, value, field)}
    </td>
  )
}

/** Reactive formula cell — calls `formula_eval` on mount and when
 *  its record's fields change. Uses a module-level cache keyed by
 *  `(expression, fields-signature)` so the same formula over
 *  identical inputs never hits the kernel twice. */
const formulaCache = new Map<string, string>()

function FormulaCell({
  expression,
  record,
  client,
}: {
  expression: string
  record: BaseRecord
  client: BasesKernelClient
}) {
  const { id: _id, ...fields } = record
  const key = useMemo(
    () => `${expression}\u0000${JSON.stringify(fields)}`,
    [expression, fields],
  )
  const [value, setValue] = useState<string | null>(() => formulaCache.get(key) ?? null)
  const [err, setErr] = useState<string | null>(null)

  useEffect(() => {
    const hit = formulaCache.get(key)
    if (hit !== undefined) {
      setValue(hit)
      setErr(null)
      return
    }
    let cancelled = false
    client
      .formulaEval(expression, fields)
      .then((display) => {
        if (cancelled) return
        formulaCache.set(key, display)
        setValue(display)
        setErr(null)
      })
      .catch((e: unknown) => {
        if (cancelled) return
        setErr(errMsg(e))
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])

  if (err) {
    return (
      <span style={{ color: 'var(--risk, #f48771)' }} title={err}>
        #err
      </span>
    )
  }
  return <span>{value ?? '…'}</span>
}

function renderReadCell(def: FieldDefinition, value: unknown, field: string): React.ReactNode {
  const s = formatValue(def.type, value)
  if (def.type === 'url' && typeof value === 'string' && value) {
    return (
      <a
        href={value}
        target="_blank"
        rel="noreferrer"
        style={{ color: 'var(--accent, #60a5fa)' }}
        onClick={(e) => e.stopPropagation()}
      >
        {s}
      </a>
    )
  }
  if (def.type === 'multi-select' && Array.isArray(value)) {
    return (
      <span>
        {value.map((v) => (
          <span
            key={`${field}:${String(v)}`}
            style={{
              display: 'inline-block',
              padding: '1px 6px',
              marginRight: 4,
              borderRadius: 4,
              background: 'var(--bg-raised, #252529)',
              fontSize: 11,
            }}
          >
            {String(v)}
          </span>
        ))}
      </span>
    )
  }
  return s || '\u00A0'
}

interface EditorProps {
  def: FieldDefinition
  value: unknown
  onCommit(value: unknown): void
  onCancel(): void
}

function CellEditor({ def, value, onCommit, onCancel }: EditorProps) {
  const editorStyle: React.CSSProperties = {
    width: '100%',
    padding: '4px 10px',
    background: 'var(--bg-input, #1e1e22)',
    color: 'var(--fg-primary, #e4e4e7)',
    border: '1px solid var(--accent, #60a5fa)',
    outline: 'none',
    fontSize: 12,
    fontFamily: 'inherit',
    boxSizing: 'border-box',
  }

  const commit = (raw: unknown) => onCommit(coerce(def.type, raw))

  switch (def.type) {
    case 'select': {
      const options = def.options ?? []
      return (
        <select
          autoFocus
          defaultValue={typeof value === 'string' ? value : ''}
          onBlur={(e) => commit(e.currentTarget.value)}
          onChange={(e) => commit(e.currentTarget.value)}
          style={editorStyle}
        >
          <option value=""></option>
          {options.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
      )
    }
    case 'multi-select': {
      const options = def.options ?? []
      const selected = new Set(Array.isArray(value) ? (value as unknown[]).map(String) : [])
      return (
        <select
          autoFocus
          multiple
          defaultValue={Array.from(selected)}
          onBlur={(e) => {
            const picks = Array.from(e.currentTarget.selectedOptions).map((o) => o.value)
            commit(picks)
          }}
          style={{ ...editorStyle, height: 'auto', minHeight: 24 }}
        >
          {options.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
      )
    }
    case 'long-text':
      return (
        <textarea
          autoFocus
          defaultValue={value == null ? '' : String(value)}
          onBlur={(e) => commit(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.preventDefault()
              onCancel()
            } else if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
              e.preventDefault()
              commit((e.currentTarget as HTMLTextAreaElement).value)
            }
          }}
          style={{ ...editorStyle, minHeight: 48, resize: 'vertical' }}
        />
      )
    default: {
      const inputType = inputTypeFor(def.type)
      return (
        <input
          autoFocus
          type={inputType}
          defaultValue={value == null ? '' : String(value)}
          onBlur={(e) => commit(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.preventDefault()
              onCancel()
            } else if (e.key === 'Enter') {
              e.preventDefault()
              commit((e.currentTarget as HTMLInputElement).value)
            }
          }}
          style={editorStyle}
        />
      )
    }
  }
}

function inputTypeFor(kind: FieldKind): string {
  switch (kind) {
    case 'number':
    case 'currency':
    case 'percent':
      return 'number'
    case 'date':
      return 'date'
    case 'time':
      return 'time'
    case 'datetime':
      return 'datetime-local'
    case 'url':
      return 'url'
    case 'email':
      return 'email'
    default:
      return 'text'
  }
}

function coerce(kind: FieldKind, raw: unknown): unknown {
  if (kind === 'number' || kind === 'currency' || kind === 'percent') {
    const s = typeof raw === 'string' ? raw : String(raw ?? '')
    if (s === '') return null
    const n = Number(s)
    return Number.isFinite(n) ? n : null
  }
  if (kind === 'multi-select') {
    return Array.isArray(raw) ? raw : []
  }
  return raw
}

function buildColumns(base: Base): Column[] {
  const entries = Object.entries(base.schema.fields ?? {}).filter(
    ([name]) => name !== 'id',
  )
  const cols = entries.map(([name, def]) => ({ name, def: parseFieldDef(def) }))
  // Primary field first; otherwise preserve declared order.
  cols.sort((a, b) => {
    if (a.def.primary === b.def.primary) return 0
    return a.def.primary ? -1 : 1
  })
  return cols
}

function sortRecords(
  records: BaseRecord[],
  sort: { field: string; dir: SortDir } | null,
  columns: Column[],
): BaseRecord[] {
  if (!sort) return records
  const col = columns.find((c) => c.name === sort.field)
  if (!col) return records
  const mult = sort.dir === 'asc' ? 1 : -1
  const out = [...records]
  out.sort((a, b) => mult * compareValues(col.def.type, a[sort.field], b[sort.field]))
  return out
}

function compareValues(kind: FieldKind, a: unknown, b: unknown): number {
  const na = a == null || a === ''
  const nb = b == null || b === ''
  if (na && nb) return 0
  if (na) return 1
  if (nb) return -1
  if (kind === 'number' || kind === 'currency' || kind === 'percent') {
    const an = Number(a)
    const bn = Number(b)
    if (Number.isFinite(an) && Number.isFinite(bn)) return an - bn
  }
  if (kind === 'checkbox') {
    return (a ? 1 : 0) - (b ? 1 : 0)
  }
  return String(a).localeCompare(String(b))
}

function errMsg(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

function filenameStem(relpath: string): string {
  const slash = Math.max(relpath.lastIndexOf('/'), relpath.lastIndexOf('\\'))
  const name = slash >= 0 ? relpath.slice(slash + 1) : relpath
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(0, dot) : name
}

const toolbarBtnStyle: React.CSSProperties = {
  padding: '3px 8px',
  background: 'var(--bg-raised, #252529)',
  color: 'var(--fg-primary, #e4e4e7)',
  border: '1px solid var(--border-subtle, #2a2a2e)',
  borderRadius: 3,
  fontSize: 11,
  cursor: 'pointer',
}
