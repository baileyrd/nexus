// Phase 3 of docs/bases-shell-plan.md — Board (Kanban) view.
// Columns key on a `select` property; drag-drop between columns
// writes the new value through `base_record_update`.

import { useMemo, useState } from 'react'
import { useBasesStore } from './basesStore'
import type { Base, BaseRecord, BasesKernelClient } from './kernelClient'
import { formatValue, parseFieldDef, type FieldDefinition } from './fieldTypes'

interface Props {
  relpath: string
  base: Base
  client: BasesKernelClient
}

interface Column {
  name: string
  def: FieldDefinition
}

const UNASSIGNED = '__unassigned__'

export function BasesBoard({ relpath, base, client }: Props) {
  const groupField = useBasesStore((s) => s.tabs[relpath]?.boardGroupField ?? null)
  const setBoardGroupField = useBasesStore((s) => s.setBoardGroupField)
  const patchRecord = useBasesStore((s) => s.patchRecord)
  const setSelectedRecordId = useBasesStore((s) => s.setSelectedRecordId)
  const pushHistory = useBasesStore((s) => s.pushHistory)

  const [opError, setOpError] = useState<string | null>(null)
  const [dragOver, setDragOver] = useState<string | null>(null)

  const selectColumns = useMemo(() => selectFields(base), [base])
  const active = useMemo<Column | null>(() => {
    if (selectColumns.length === 0) return null
    const chosen = groupField
      ? selectColumns.find((c) => c.name === groupField) ?? selectColumns[0]
      : selectColumns[0]
    return chosen
  }, [groupField, selectColumns])

  const otherColumns = useMemo(() => otherFields(base), [base])

  const groups = useMemo(() => {
    if (!active) return []
    const buckets = new Map<string, BaseRecord[]>()
    const options = active.def.options ?? []
    for (const opt of options) buckets.set(opt, [])
    buckets.set(UNASSIGNED, [])
    for (const r of base.records) {
      const raw = r[active.name]
      const key = typeof raw === 'string' && buckets.has(raw) ? raw : UNASSIGNED
      buckets.get(key)?.push(r)
    }
    return Array.from(buckets.entries()).map(([key, records]) => ({ key, records }))
  }, [active, base.records])

  if (selectColumns.length === 0) {
    return (
      <div
        style={{
          flex: 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: 'var(--text-muted)',
          padding: 24,
          fontSize: 12,
        }}
      >
        The Board view needs at least one <code style={{ margin: '0 4px' }}>select</code> field.
      </div>
    )
  }

  const moveRecord = async (recordId: string, toKey: string) => {
    if (!active) return
    const value = toKey === UNASSIGNED ? null : toKey
    // Mirror the BasesTable cell-edit pattern (BasesTable.tsx:71-99)
    // — capture the pre-edit value so the inverse rolls the column
    // back to exactly what the kernel had before the drop.
    const fieldName = active.name
    const prev = useBasesStore
      .getState()
      .tabs[relpath]?.base?.records.find((r) => r.id === recordId)?.[fieldName]
    try {
      setOpError(null)
      await client.updateRecord(relpath, recordId, { [fieldName]: value })
      patchRecord(relpath, recordId, { [fieldName]: value })
      pushHistory(relpath, {
        label: `Move card to ${toKey === UNASSIGNED ? 'Unassigned' : toKey}`,
        forward: async () => {
          await client.updateRecord(relpath, recordId, { [fieldName]: value })
          patchRecord(relpath, recordId, { [fieldName]: value })
        },
        inverse: async () => {
          await client.updateRecord(relpath, recordId, { [fieldName]: prev })
          patchRecord(relpath, recordId, { [fieldName]: prev })
        },
      })
    } catch (err) {
      setOpError(`move failed: ${errMsg(err)}`)
    }
  }

  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '6px 12px',
          borderBottom: '1px solid var(--background-modifier-border)',
          fontSize: 12,
          color: 'var(--text-muted)',
        }}
      >
        <span>Group by</span>
        <select
          value={active?.name ?? ''}
          onChange={(e) => setBoardGroupField(relpath, e.target.value || null)}
          style={{
            background: 'var(--background-secondary)',
            color: 'var(--text-normal)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 3,
            padding: '2px 6px',
            fontSize: 11,
          }}
        >
          {selectColumns.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name}
            </option>
          ))}
        </select>
        {opError && <span style={{ color: 'var(--risk)' }}>{opError}</span>}
      </div>
      <div
        style={{
          flex: 1,
          display: 'flex',
          gap: 8,
          padding: 12,
          overflowX: 'auto',
          overflowY: 'hidden',
          alignItems: 'stretch',
        }}
      >
        {groups.map(({ key, records }) => (
          <BoardColumn
            key={key}
            columnKey={key}
            records={records}
            otherColumns={otherColumns}
            isDragOver={dragOver === key}
            onDragOver={(over) => setDragOver(over ? key : null)}
            onDrop={(recordId) => {
              setDragOver(null)
              void moveRecord(recordId, key)
            }}
            onSelect={(id) => setSelectedRecordId(relpath, id)}
          />
        ))}
      </div>
    </div>
  )
}

interface BoardColumnProps {
  columnKey: string
  records: BaseRecord[]
  otherColumns: Column[]
  isDragOver: boolean
  onDragOver(over: boolean): void
  onDrop(recordId: string): void
  onSelect(recordId: string): void
}

function BoardColumn({
  columnKey,
  records,
  otherColumns,
  isDragOver,
  onDragOver,
  onDrop,
  onSelect,
}: BoardColumnProps) {
  const label = columnKey === UNASSIGNED ? 'Unassigned' : columnKey
  return (
    <div
      style={{
        flex: '0 0 260px',
        display: 'flex',
        flexDirection: 'column',
        background: 'var(--background-secondary)',
        border: `1px solid ${isDragOver ? 'var(--interactive-accent)' : 'var(--background-modifier-border)'}`,
        borderRadius: 6,
        minWidth: 0,
      }}
      onDragOver={(e) => {
        e.preventDefault()
        e.dataTransfer.dropEffect = 'move'
        if (!isDragOver) onDragOver(true)
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
          onDragOver(false)
        }
      }}
      onDrop={(e) => {
        e.preventDefault()
        const id = e.dataTransfer.getData('text/plain')
        if (id) onDrop(id)
      }}
    >
      <div
        style={{
          padding: '8px 12px',
          fontSize: 11,
          fontWeight: 600,
          color: columnKey === UNASSIGNED ? 'var(--text-faint)' : 'var(--text-normal)',
          borderBottom: '1px solid var(--background-modifier-border)',
          display: 'flex',
          justifyContent: 'space-between',
        }}
      >
        <span>{label}</span>
        <span style={{ color: 'var(--text-faint)' }}>{records.length}</span>
      </div>
      <div
        style={{
          flex: 1,
          overflowY: 'auto',
          padding: 8,
          display: 'flex',
          flexDirection: 'column',
          gap: 6,
        }}
      >
        {records.map((r) => (
          <Card
            key={r.id}
            record={r}
            otherColumns={otherColumns}
            onSelect={() => onSelect(r.id)}
          />
        ))}
      </div>
    </div>
  )
}

function Card({
  record,
  otherColumns,
  onSelect,
}: {
  record: BaseRecord
  otherColumns: Column[]
  onSelect(): void
}) {
  const primary = otherColumns.find((c) => c.def.primary) ?? otherColumns[0]
  const rest = otherColumns.filter((c) => c !== primary).slice(0, 3)
  const title = primary ? formatValue(primary.def.type, record[primary.name]) : record.id
  return (
    <div
      draggable
      onDragStart={(e) => e.dataTransfer.setData('text/plain', record.id)}
      onClick={onSelect}
      style={{
        padding: 8,
        background: 'var(--background-primary)',
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 4,
        fontSize: 12,
        cursor: 'grab',
        color: 'var(--text-normal)',
      }}
    >
      <div style={{ fontWeight: 500, marginBottom: rest.length ? 4 : 0 }}>
        {title || <span style={{ color: 'var(--text-faint)' }}>Untitled</span>}
      </div>
      {rest.map((c) => {
        const v = record[c.name]
        if (v == null || v === '') return null
        return (
          <div
            key={c.name}
            style={{
              color: 'var(--text-muted)',
              fontSize: 11,
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            <span style={{ color: 'var(--text-faint)' }}>{c.name}: </span>
            {formatValue(c.def.type, v)}
          </div>
        )
      })}
    </div>
  )
}

function selectFields(base: Base): Column[] {
  return Object.entries(base.schema.fields ?? {})
    .map(([name, def]) => ({ name, def: parseFieldDef(def) }))
    .filter((c) => c.def.type === 'select')
}

function otherFields(base: Base): Column[] {
  return Object.entries(base.schema.fields ?? {})
    .filter(([name]) => name !== 'id')
    .map(([name, def]) => ({ name, def: parseFieldDef(def) }))
}

function errMsg(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}
