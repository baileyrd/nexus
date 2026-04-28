// Schema-editor side panel for `.bases`. Lists every column in the
// loaded schema and exposes rename / retype / required toggle /
// delete / formula-expression edit. Mutations go through the base_*
// kernel handlers and then re-load the base so the table picks up
// the new fields immediately.
//
// Rename and retype are two of the deferred Phase-6 items from the
// plan (see BACKLOG). Rename uses `base_property_rename`; retype
// hits `base_property_update` with `migrate_values=true` when the
// target type differs, so existing record values get coerced on the
// kernel side. Both prompt the user when records exist to confirm
// the migration.

import { useEffect, useMemo, useState } from 'react'
import type { Base, BaseRecord, BasesKernelClient } from './kernelClient'
import { useBasesStore } from './basesStore'
import {
  isReadOnly,
  parseFieldDef,
  type FieldDefinition,
  type FieldKind,
} from './fieldTypes'
import { getBasesApi } from './runtime'

/** Refusal threshold for destructive schema mutations. Snapshotting
 *  cell values for every record on a base with more rows than this
 *  would balloon the history entry into the megabytes; we surface a
 *  "history truncated" hint instead of silently losing data. */
const SCHEMA_HISTORY_RECORD_LIMIT = 50_000

const ALL_KINDS: FieldKind[] = [
  'text',
  'long-text',
  'number',
  'currency',
  'percent',
  'checkbox',
  'date',
  'time',
  'datetime',
  'select',
  'multi-select',
  'url',
  'email',
  'formula',
]

interface Props {
  relpath: string
  base: Base
  client: BasesKernelClient
}

export function SchemaEditor({ relpath, base, client }: Props) {
  const setBase = useBasesStore((s) => s.setBase)
  const setOpen = useBasesStore((s) => s.setSchemaEditorOpen)
  const pushHistory = useBasesStore((s) => s.pushHistory)
  const setLastUndoError = useBasesStore((s) => s.setLastUndoError)
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const columns = useMemo(() => {
    return Object.entries(base.schema.fields)
      .filter(([n]) => n !== 'id')
      .map(([name, def]) => ({ name, def: parseFieldDef(def) }))
  }, [base.schema.fields])

  const hasRecords = base.records.length > 0

  const reload = async () => {
    const fresh = await client.loadBase(relpath)
    setBase(relpath, fresh)
  }

  /** Run a mutation, reload, then optionally record a history entry.
   *  When `historyEntryFor` is provided, it's invoked with the kernel
   *  result so callers can capture pre-state for the inverse and
   *  build the entry only after the forward succeeded. Returning
   *  `null` from it skips the push (used by destructive mutations
   *  that exceed the snapshot threshold). */
  const runOp = async <T,>(
    fn: () => Promise<T>,
    historyEntryFor?: (result: T) => {
      label: string
      forward(): Promise<void>
      inverse(): Promise<void>
    } | null,
  ): Promise<T | null> => {
    try {
      setBusy(true)
      setErr(null)
      const r = await fn()
      await reload()
      if (historyEntryFor) {
        const entry = historyEntryFor(r)
        if (entry) pushHistory(relpath, entry)
      }
      return r
    } catch (e) {
      setErr((e as Error).message ?? String(e))
      return null
    } finally {
      setBusy(false)
    }
  }

  const handleRename = async (oldName: string) => {
    const api = getBasesApi()
    if (!api) return
    const next = await api.input.prompt(`Rename "${oldName}" to:`)
    if (!next) return
    const trimmed = next.trim()
    if (!trimmed || trimmed === oldName) return
    if (trimmed === 'id') {
      setErr('"id" is reserved.')
      return
    }
    if (hasRecords) {
      const ok = await api.input.confirm(
        `Rename column "${oldName}" to "${trimmed}" and move values on all ${base.records.length} records?`,
      )
      if (!ok) return
    }
    await runOp(
      () => client.renameProperty(relpath, oldName, trimmed),
      () => ({
        label: `Rename column ${oldName} → ${trimmed}`,
        forward: async () => {
          await client.renameProperty(relpath, oldName, trimmed)
          await reload()
        },
        // Inverse renames the new name back to the old one.
        inverse: async () => {
          await client.renameProperty(relpath, trimmed, oldName)
          await reload()
        },
      }),
    )
  }

  const handleRetype = async (name: string, def: FieldDefinition, nextKind: FieldKind) => {
    if (nextKind === def.type) return
    const api = getBasesApi()
    if (!api) return
    if (hasRecords) {
      const ok = await api.input.confirm(
        `Change "${name}" from ${def.type} to ${nextKind}? Values on ${base.records.length} records will be coerced (uncoercible values drop to blank).`,
      )
      if (!ok) return
    }
    // Snapshot pre-retype cell values for every record so the
    // inverse can restore them after the kernel coerces. We capture
    // BEFORE the kernel call so racing events don't bleed into the
    // snapshot.
    if (base.records.length > SCHEMA_HISTORY_RECORD_LIMIT) {
      setLastUndoError(
        relpath,
        `history truncated: retype on bases with more than ${SCHEMA_HISTORY_RECORD_LIMIT.toLocaleString()} records is not undoable`,
      )
      const nextDefRefuse: Record<string, unknown> = { ...def, type: nextKind }
      if (nextKind !== 'select' && nextKind !== 'multi-select') {
        delete nextDefRefuse.options
      } else if (!Array.isArray(nextDefRefuse.options)) {
        nextDefRefuse.options = []
      }
      if (nextKind !== 'formula') delete nextDefRefuse.expression
      await runOp(() => client.updateProperty(relpath, name, nextDefRefuse, true))
      return
    }
    const prevDef: Record<string, unknown> = { ...def }
    const prevValues = new Map<string, unknown>()
    for (const r of base.records) {
      prevValues.set(r.id, r[name])
    }
    const nextDef: Record<string, unknown> = { ...def, type: nextKind }
    if (nextKind !== 'select' && nextKind !== 'multi-select') {
      delete nextDef.options
    } else if (!Array.isArray(nextDef.options)) {
      nextDef.options = []
    }
    if (nextKind !== 'formula') {
      delete nextDef.expression
    }
    await runOp(
      () => client.updateProperty(relpath, name, nextDef, true),
      () => ({
        label: `Retype ${name} (${def.type} → ${nextKind})`,
        forward: async () => {
          await client.updateProperty(relpath, name, nextDef, true)
          await reload()
        },
        // Inverse: restore the prior definition with migrate=true so
        // the kernel re-coerces, then write each pre-retype value back
        // verbatim. The second pass is what makes the round-trip
        // lossless even when coercion silently dropped data.
        inverse: async () => {
          await client.updateProperty(relpath, name, prevDef, true)
          for (const [recordId, value] of prevValues) {
            await client.updateRecord(relpath, recordId, { [name]: value })
          }
          await reload()
        },
      }),
    )
  }

  const handleToggleRequired = async (name: string, def: FieldDefinition) => {
    const prevDef: Record<string, unknown> = { ...def }
    const nextDef: Record<string, unknown> = { ...def, required: !def.required }
    await runOp(
      () => client.updateProperty(relpath, name, nextDef, false),
      () => ({
        label: `Toggle required on ${name}`,
        forward: async () => {
          await client.updateProperty(relpath, name, nextDef, false)
          await reload()
        },
        inverse: async () => {
          await client.updateProperty(relpath, name, prevDef, false)
          await reload()
        },
      }),
    )
  }

  const handleUpdateOptions = async (name: string, def: FieldDefinition, optionsCsv: string) => {
    const options = optionsCsv
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
    const prevDef: Record<string, unknown> = { ...def }
    const nextDef: Record<string, unknown> = { ...def, options }
    await runOp(
      () => client.updateProperty(relpath, name, nextDef, false),
      () => ({
        label: `Update options on ${name}`,
        forward: async () => {
          await client.updateProperty(relpath, name, nextDef, false)
          await reload()
        },
        inverse: async () => {
          await client.updateProperty(relpath, name, prevDef, false)
          await reload()
        },
      }),
    )
  }

  const handleUpdateExpression = async (name: string, def: FieldDefinition, expression: string) => {
    const prevDef: Record<string, unknown> = { ...def }
    const nextDef: Record<string, unknown> = { ...def, expression }
    await runOp(
      () => client.updateProperty(relpath, name, nextDef, false),
      () => ({
        label: `Update expression on ${name}`,
        forward: async () => {
          await client.updateProperty(relpath, name, nextDef, false)
          await reload()
        },
        inverse: async () => {
          await client.updateProperty(relpath, name, prevDef, false)
          await reload()
        },
      }),
    )
  }

  const handleDelete = async (name: string) => {
    const api = getBasesApi()
    if (!api) return
    const ok = await api.input.confirm(
      `Delete column "${name}"? Values on every record will be lost.`,
    )
    if (!ok) return
    // Refuse to push a destructive history entry on huge bases; the
    // snapshot would balloon into MB. Forward still runs; the user is
    // told via the lastUndoError banner that the action isn't undoable.
    if (base.records.length > SCHEMA_HISTORY_RECORD_LIMIT) {
      setLastUndoError(
        relpath,
        `history truncated: delete column on bases with more than ${SCHEMA_HISTORY_RECORD_LIMIT.toLocaleString()} records is not undoable`,
      )
      await runOp(() => client.deleteProperty(relpath, name))
      return
    }
    const prevDef: Record<string, unknown> = {
      ...(base.schema.fields[name] as Record<string, unknown>),
    }
    const prevValues = new Map<string, unknown>()
    for (const r of base.records) {
      if (name in r) prevValues.set(r.id, r[name])
    }
    await runOp(
      () => client.deleteProperty(relpath, name),
      () => ({
        label: `Delete column ${name}`,
        forward: async () => {
          await client.deleteProperty(relpath, name)
          await reload()
        },
        // Recreate column then restore every snapshotted value.
        inverse: async () => {
          await client.createProperty(relpath, name, prevDef)
          for (const [recordId, value] of prevValues) {
            await client.updateRecord(relpath, recordId, { [name]: value })
          }
          await reload()
        },
      }),
    )
  }

  const handleAdd = async () => {
    const api = getBasesApi()
    if (!api) return
    const name = await api.input.prompt('New column name:')
    if (!name) return
    const trimmed = name.trim()
    if (!trimmed) return
    if (trimmed === 'id' || base.schema.fields[trimmed]) {
      setErr(`"${trimmed}" is already in use.`)
      return
    }
    const newDef: Record<string, unknown> = { type: 'text' }
    await runOp(
      () => client.createProperty(relpath, trimmed, newDef),
      () => ({
        label: `Add column ${trimmed}`,
        forward: async () => {
          await client.createProperty(relpath, trimmed, newDef)
          await reload()
        },
        inverse: async () => {
          await client.deleteProperty(relpath, trimmed)
          await reload()
        },
      }),
    )
  }

  return (
    <div
      style={{
        width: 360,
        flexShrink: 0,
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        borderLeft: '1px solid var(--border-subtle, #2a2a2e)',
        background: 'var(--bg-raised, #252529)',
        fontSize: 12,
      }}
    >
      <div
        style={{
          padding: '8px 12px',
          borderBottom: '1px solid var(--border-subtle, #2a2a2e)',
          display: 'flex',
          alignItems: 'center',
          gap: 8,
        }}
      >
        <strong style={{ color: 'var(--fg-primary, #e4e4e7)' }}>Schema</strong>
        <span style={{ color: 'var(--fg-muted, #9ca3af)' }}>
          {columns.length} {columns.length === 1 ? 'column' : 'columns'}
        </span>
        <div style={{ flex: 1 }} />
        <button
          type="button"
          onClick={() => setOpen(relpath, false)}
          style={closeBtnStyle}
          title="Close schema editor"
        >
          ×
        </button>
      </div>
      <div style={{ flex: 1, overflow: 'auto', padding: 8, display: 'flex', flexDirection: 'column', gap: 8 }}>
        {err && (
          <div style={{ color: 'var(--risk, #f48771)', padding: '4px 8px' }}>{err}</div>
        )}
        {columns.map(({ name, def }) => (
          <PropertyRow
            key={name}
            name={name}
            def={def}
            busy={busy}
            client={client}
            recordSample={base.records[0]}
            onRename={() => void handleRename(name)}
            onRetype={(k) => void handleRetype(name, def, k)}
            onToggleRequired={() => void handleToggleRequired(name, def)}
            onUpdateOptions={(csv) => void handleUpdateOptions(name, def, csv)}
            onUpdateExpression={(expr) => void handleUpdateExpression(name, def, expr)}
            onDelete={() => void handleDelete(name)}
          />
        ))}
        <button
          type="button"
          disabled={busy}
          onClick={() => void handleAdd()}
          style={{
            padding: '6px 10px',
            background: 'var(--bg-primary, #1e1e1e)',
            color: 'var(--fg-primary, #e4e4e7)',
            border: '1px dashed var(--border-subtle, #2a2a2e)',
            borderRadius: 4,
            cursor: busy ? 'not-allowed' : 'pointer',
            opacity: busy ? 0.5 : 1,
          }}
        >
          + Add column
        </button>
      </div>
    </div>
  )
}

interface RowProps {
  name: string
  def: FieldDefinition
  busy: boolean
  client: BasesKernelClient
  recordSample: BaseRecord | undefined
  onRename: () => void
  onRetype: (kind: FieldKind) => void
  onToggleRequired: () => void
  onUpdateOptions: (csv: string) => void
  onUpdateExpression: (expression: string) => void
  onDelete: () => void
}

function PropertyRow({
  name,
  def,
  busy,
  client,
  recordSample,
  onRename,
  onRetype,
  onToggleRequired,
  onUpdateOptions,
  onUpdateExpression,
  onDelete,
}: RowProps) {
  const [optionsDraft, setOptionsDraft] = useState((def.options ?? []).join(', '))
  const [expressionDraft, setExpressionDraft] = useState(def.expression ?? '')
  const readOnly = isReadOnly(def.type)

  return (
    <div
      style={{
        padding: 8,
        background: 'var(--bg-primary, #1e1e1e)',
        border: '1px solid var(--border-subtle, #2a2a2e)',
        borderRadius: 4,
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <span style={{ fontWeight: 500, color: 'var(--fg-primary, #e4e4e7)' }}>{name}</span>
        {def.primary && (
          <span style={{ color: 'var(--accent, #60a5fa)', fontSize: 10 }}>primary</span>
        )}
        <div style={{ flex: 1 }} />
        <button type="button" disabled={busy} onClick={onRename} style={rowBtnStyle} title="Rename column">
          rename
        </button>
        <button
          type="button"
          disabled={busy || def.primary}
          onClick={onDelete}
          style={{
            ...rowBtnStyle,
            color: 'var(--risk, #f48771)',
            opacity: def.primary ? 0.4 : 1,
          }}
          title={def.primary ? "Can't delete the primary column" : 'Delete column'}
        >
          delete
        </button>
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <label style={{ color: 'var(--fg-muted, #9ca3af)' }}>type</label>
        <select
          disabled={busy}
          value={def.type}
          onChange={(e) => onRetype(e.currentTarget.value as FieldKind)}
          style={selectStyle}
        >
          {ALL_KINDS.map((k) => (
            <option key={k} value={k}>
              {k}
            </option>
          ))}
        </select>
        {!readOnly && (
          <label style={{ display: 'flex', alignItems: 'center', gap: 4, color: 'var(--fg-muted, #9ca3af)' }}>
            <input
              type="checkbox"
              disabled={busy}
              checked={def.required === true}
              onChange={onToggleRequired}
            />
            required
          </label>
        )}
      </div>
      {(def.type === 'select' || def.type === 'multi-select') && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <label style={{ color: 'var(--fg-muted, #9ca3af)' }}>options (comma-separated)</label>
          <input
            type="text"
            disabled={busy}
            value={optionsDraft}
            onChange={(e) => setOptionsDraft(e.currentTarget.value)}
            onBlur={() => {
              const next = (def.options ?? []).join(', ')
              if (optionsDraft !== next) onUpdateOptions(optionsDraft)
            }}
            style={inputStyle}
          />
        </div>
      )}
      {def.type === 'formula' && (
        <FormulaEditorRow
          value={expressionDraft}
          onChange={setExpressionDraft}
          onCommit={() => {
            if (expressionDraft !== (def.expression ?? '')) onUpdateExpression(expressionDraft)
          }}
          sample={recordSample}
          client={client}
          busy={busy}
        />
      )}
    </div>
  )
}

/** Live-preview formula editor. Evaluates the current draft against
 *  the first record of the base (if any) so the user can see what
 *  the expression resolves to before committing. */
function FormulaEditorRow({
  value,
  onChange,
  onCommit,
  sample,
  client,
  busy,
}: {
  value: string
  onChange: (v: string) => void
  onCommit: () => void
  sample: BaseRecord | undefined
  client: BasesKernelClient
  busy: boolean
}) {
  const [preview, setPreview] = useState<string>('')
  const [previewErr, setPreviewErr] = useState<string | null>(null)

  // Evaluate on value change (debounced). A tiny delay keeps the
  // kernel roundtrip from running on every keystroke for nothing.
  useEffect(() => {
    setPreview('')
    setPreviewErr(null)
    if (!value || !sample) return
    let cancelled = false
    const handle = setTimeout(() => {
      const { id: _id, ...fields } = sample
      client
        .formulaEval(value, fields)
        .then((display) => {
          if (cancelled) return
          setPreview(display)
          setPreviewErr(null)
        })
        .catch((e: unknown) => {
          if (cancelled) return
          setPreviewErr(e instanceof Error ? e.message : String(e))
        })
    }, 200)
    return () => {
      cancelled = true
      clearTimeout(handle)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <label style={{ color: 'var(--fg-muted, #9ca3af)' }}>expression</label>
      <textarea
        disabled={busy}
        value={value}
        onChange={(e) => onChange(e.currentTarget.value)}
        onBlur={onCommit}
        onKeyDown={(e) => {
          if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
            e.preventDefault()
            onCommit()
          }
        }}
        rows={2}
        spellCheck={false}
        placeholder={'e.g. price * 1.1'}
        style={{ ...inputStyle, fontFamily: 'var(--f-mono, monospace)', resize: 'vertical' }}
      />
      <div style={{ color: 'var(--fg-muted, #9ca3af)', fontSize: 11 }}>
        Preview (first record):{' '}
        {previewErr ? (
          <span style={{ color: 'var(--risk, #f48771)' }}>error — {previewErr}</span>
        ) : value ? (
          <span style={{ color: 'var(--fg-primary, #e4e4e7)' }}>{preview || '…'}</span>
        ) : (
          <span>—</span>
        )}
      </div>
    </div>
  )
}

const rowBtnStyle: React.CSSProperties = {
  padding: '2px 8px',
  background: 'var(--bg-raised, #252529)',
  color: 'var(--fg-primary, #e4e4e7)',
  border: '1px solid var(--border-subtle, #2a2a2e)',
  borderRadius: 3,
  fontSize: 11,
  cursor: 'pointer',
}

const closeBtnStyle: React.CSSProperties = {
  padding: '2px 8px',
  background: 'transparent',
  color: 'var(--fg-muted, #9ca3af)',
  border: 'none',
  fontSize: 16,
  cursor: 'pointer',
  lineHeight: 1,
}

const selectStyle: React.CSSProperties = {
  padding: '2px 6px',
  background: 'var(--bg-raised, #252529)',
  color: 'var(--fg-primary, #e4e4e7)',
  border: '1px solid var(--border-subtle, #2a2a2e)',
  borderRadius: 3,
  fontSize: 11,
}

const inputStyle: React.CSSProperties = {
  padding: '4px 8px',
  background: 'var(--bg-raised, #252529)',
  color: 'var(--fg-primary, #e4e4e7)',
  border: '1px solid var(--border-subtle, #2a2a2e)',
  borderRadius: 3,
  fontSize: 12,
  fontFamily: 'inherit',
  outline: 'none',
}
