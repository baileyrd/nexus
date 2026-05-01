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
import {
  cellsToTsv,
  coerceValue,
  isPasteable,
  normalizeRange,
  parseTsv,
  rangeContains,
  readClipboardPayload,
  rowsToTsv,
  writeClipboardPayload,
  type CellRange,
  type CellsPayload,
  type RowsPayload,
} from './clipboard'
import { contextKeyService } from '../../../host/ContextKeyService'
import { setActiveTableClipboard } from './tableClipboard'

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
  const cellSelection = useBasesStore((s) => s.tabs[relpath]?.cellSelection ?? null)
  const setSort = useBasesStore((s) => s.setSort)
  const setSelectedRecordId = useBasesStore((s) => s.setSelectedRecordId)
  const setCellSelection = useBasesStore((s) => s.setCellSelection)
  const patchRecord = useBasesStore((s) => s.patchRecord)
  const appendRecord = useBasesStore((s) => s.appendRecord)
  const removeRecord = useBasesStore((s) => s.removeRecord)
  const pushHistory = useBasesStore((s) => s.pushHistory)
  const undo = useBasesStore((s) => s.undo)
  const redo = useBasesStore((s) => s.redo)

  const [editing, setEditing] = useState<{ id: string; field: string } | null>(null)
  const [opError, setOpError] = useState<string | null>(null)
  // Anchor for shift-click range extension. Distinct from
  // `cellSelection` so a single-cell click after a range still has
  // the prior anchor available for the next shift-click.
  const anchorRef = useRef<{ row: number; col: number } | null>(null)

  // Keep the global `bases.editing` context key in sync — the cell
  // clipboard keybindings gate on `bases.focused && !bases.editing`
  // so that Mod-V inside a CellEditor inserts text instead of
  // triggering a paste.
  useEffect(() => {
    contextKeyService.set('bases.editing', !!editing)
    return () => {
      contextKeyService.set('bases.editing', false)
    }
  }, [editing])

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

  // Cell-range click handling (BL-031). A plain click anchors a
  // single-cell selection; shift+click extends the range from the
  // existing anchor (or the most recent click if no anchor is set).
  const handleCellClick = useCallback(
    (rowIndex: number, colIndex: number, shiftKey: boolean) => {
      const anchor = anchorRef.current
      if (shiftKey && anchor) {
        setCellSelection(relpath, normalizeRange(anchor, { row: rowIndex, col: colIndex }))
        return
      }
      anchorRef.current = { row: rowIndex, col: colIndex }
      setCellSelection(relpath, { r1: rowIndex, c1: colIndex, r2: rowIndex, c2: colIndex })
      // Mirror the row id onto the selection model so the row-delete
      // / arrow-key paths still see the right record. We don't go
      // through `setSelectedRecordId` (which would null out the
      // cell selection); instead the table reads cell selection
      // first and falls back to the row id only when none is set.
    },
    [relpath, setCellSelection],
  )

  // ── BL-031 clipboard surface ───────────────────────────────────
  // Read records / columns / selection lazily inside each handler so
  // they always see the latest store state — the activeTableClipboard
  // registration lasts as long as the table is mounted, and capturing
  // a stale closure here would copy the wrong cells after the user
  // edits a row.
  const handleCopy = useCallback(async () => {
    const tab = useBasesStore.getState().tabs[relpath]
    if (!tab?.base) return
    const sel = tab.cellSelection
    const cols = buildColumns(tab.base)
    const recs = sortRecords(tab.base.records, tab.sort, cols)
    if (sel) {
      const cells: unknown[][] = []
      const fields: { name: string; type: FieldKind }[] = []
      for (let c = sel.c1; c <= sel.c2; c++) {
        const col = cols[c]
        if (col) fields.push({ name: col.name, type: col.def.type })
      }
      for (let r = sel.r1; r <= sel.r2; r++) {
        const rec = recs[r]
        if (!rec) continue
        const row: unknown[] = []
        for (let c = sel.c1; c <= sel.c2; c++) {
          const col = cols[c]
          row.push(col ? rec[col.name] : null)
        }
        cells.push(row)
      }
      const payload: CellsPayload = { kind: 'cells', fields, rows: cells }
      const tsv = cellsToTsv(cells, fields)
      try {
        await writeClipboardPayload(payload, tsv)
      } catch (err) {
        setOpError(`copy failed: ${errMsg(err)}`)
      }
      return
    }
    if (tab.selectedRecordId) {
      const rec = recs.find((r) => r.id === tab.selectedRecordId)
      if (!rec) return
      const fields = cols.map((c) => ({ name: c.name, type: c.def.type }))
      const payload: RowsPayload = { kind: 'rows', fields, records: [rec] }
      const tsv = rowsToTsv([rec], fields)
      try {
        await writeClipboardPayload(payload, tsv)
      } catch (err) {
        setOpError(`copy failed: ${errMsg(err)}`)
      }
    }
  }, [relpath])

  const handleCut = useCallback(async () => {
    if (readOnly) return
    const tab = useBasesStore.getState().tabs[relpath]
    if (!tab?.base) return
    const sel = tab.cellSelection
    if (!sel) {
      // Row cut isn't in v1 — Backspace already covers that path
      // (and a row cut would conflict with the soft-delete confirm).
      // Plain copy keeps the surface useful for "cut for clipboard"
      // muscle memory; the user just needs Backspace to delete.
      await handleCopy()
      return
    }
    await handleCopy()
    // Clear the selected cells with a single HistoryEntry. Read-only
    // / formula columns are skipped — they're never editable, so
    // they shouldn't take part in the cut.
    const cols = buildColumns(tab.base)
    const recs = sortRecords(tab.base.records, tab.sort, cols)
    interface PriorEdit { recordId: string; field: string; prev: unknown }
    const edits: PriorEdit[] = []
    for (let r = sel.r1; r <= sel.r2; r++) {
      const rec = recs[r]
      if (!rec) continue
      for (let c = sel.c1; c <= sel.c2; c++) {
        const col = cols[c]
        if (!col || !isPasteable(col.def)) continue
        edits.push({ recordId: rec.id, field: col.name, prev: rec[col.name] ?? null })
      }
    }
    if (edits.length === 0) return
    const apply = async () => {
      // Group by record so we issue one update per row, not one per
      // cell — avoids N kernel round-trips for a large cut.
      const byRecord = new Map<string, Record<string, unknown>>()
      for (const e of edits) {
        const m = byRecord.get(e.recordId) ?? {}
        m[e.field] = null
        byRecord.set(e.recordId, m)
      }
      for (const [recordId, fields] of byRecord) {
        await client.updateRecord(relpath, recordId, fields)
        patchRecord(relpath, recordId, fields)
      }
    }
    const revert = async () => {
      const byRecord = new Map<string, Record<string, unknown>>()
      for (const e of edits) {
        const m = byRecord.get(e.recordId) ?? {}
        m[e.field] = e.prev
        byRecord.set(e.recordId, m)
      }
      for (const [recordId, fields] of byRecord) {
        await client.updateRecord(relpath, recordId, fields)
        patchRecord(relpath, recordId, fields)
      }
    }
    try {
      setOpError(null)
      await apply()
      pushHistory(relpath, {
        label: edits.length === 1 ? 'Cut cell' : `Cut ${edits.length} cells`,
        forward: apply,
        inverse: revert,
      })
    } catch (err) {
      setOpError(`cut failed: ${errMsg(err)}`)
    }
  }, [client, handleCopy, patchRecord, pushHistory, readOnly, relpath])

  const handlePaste = useCallback(async () => {
    if (readOnly) return
    const tab = useBasesStore.getState().tabs[relpath]
    if (!tab?.base) return
    const cols = buildColumns(tab.base)
    const recs = sortRecords(tab.base.records, tab.sort, cols)
    const sel = tab.cellSelection
    let read: Awaited<ReturnType<typeof readClipboardPayload>>
    try {
      read = await readClipboardPayload()
    } catch (err) {
      setOpError(`paste failed: ${errMsg(err)}`)
      return
    }

    let coercedCount = 0
    interface PriorEdit { recordId: string; field: string; prev: unknown; next: unknown }
    const edits: PriorEdit[] = []
    interface CreatedRow { record: BaseRecord }
    const created: CreatedRow[] = []

    if (sel) {
      // Range / single-cell paste. Source matrix is either the typed
      // payload (preferred) or parsed TSV (external app fallback).
      let matrix: unknown[][] = []
      let sourceFields: { name: string; type: FieldKind }[] = []
      if (read.payload?.kind === 'cells') {
        matrix = read.payload.rows
        sourceFields = read.payload.fields
      } else if (read.payload?.kind === 'rows') {
        // Pasting a `rows` payload over a cell range — flatten to
        // the visible field order so it lands as cells.
        sourceFields = read.payload.fields
        matrix = read.payload.records.map((rec) =>
          sourceFields.map((f) => rec[f.name] ?? null),
        )
      } else if (read.text) {
        matrix = parseTsv(read.text)
      }
      if (matrix.length === 0) return
      const rangeRows = sel.r2 - sel.r1 + 1
      const rangeCols = sel.c2 - sel.c1 + 1
      // Tile when the range is larger than the source. Single-cell
      // selection (1×1) treats the source as anchor + extent.
      const isSingleCell = rangeRows === 1 && rangeCols === 1
      const targetRows = isSingleCell ? matrix.length : rangeRows
      const targetCols = isSingleCell ? Math.max(...matrix.map((r) => r.length)) : rangeCols
      for (let dr = 0; dr < targetRows; dr++) {
        const recIdx = sel.r1 + dr
        const rec = recs[recIdx]
        if (!rec) break
        for (let dc = 0; dc < targetCols; dc++) {
          const colIdx = sel.c1 + dc
          const col = cols[colIdx]
          if (!col || !isPasteable(col.def)) continue
          const srcRow = matrix[dr % matrix.length]
          if (!srcRow) continue
          const srcVal = srcRow[dc % Math.max(srcRow.length, 1)]
          const sourceField = sourceFields[dc % Math.max(sourceFields.length, 1)]
          const sourceKind = sourceField?.type
          const [next, didCoerce] = coerceValue(col.def.type, srcVal, sourceKind)
          if (didCoerce) coercedCount += 1
          edits.push({ recordId: rec.id, field: col.name, prev: rec[col.name] ?? null, next })
        }
      }
    } else if (tab.selectedRecordId) {
      // Single-row paste — shallow merge field-by-field by name, with
      // coercion. Useful for "duplicate record" workflows from the
      // typed `rows` payload.
      const rec = recs.find((r) => r.id === tab.selectedRecordId)
      if (!rec) return
      let sourceRecord: BaseRecord | null = null
      let sourceFields: { name: string; type: FieldKind }[] = []
      if (read.payload?.kind === 'rows' && read.payload.records[0]) {
        sourceRecord = read.payload.records[0]
        sourceFields = read.payload.fields
      } else if (read.payload?.kind === 'cells' && read.payload.rows[0]) {
        sourceFields = read.payload.fields
        const firstRow = read.payload.rows[0]
        const synthetic: BaseRecord = { id: rec.id }
        for (let i = 0; i < sourceFields.length; i++) {
          synthetic[sourceFields[i].name] = firstRow[i]
        }
        sourceRecord = synthetic
      } else if (read.text) {
        // TSV with header row — parseTsv first row treated as
        // headers when the source has no JSON typing.
        const parsed = parseTsv(read.text)
        if (parsed.length >= 2) {
          const header = parsed[0]
          const firstRow = parsed[1]
          sourceFields = header.map((name) => ({ name, type: 'text' as FieldKind }))
          const synthetic: BaseRecord = { id: rec.id }
          for (let i = 0; i < header.length; i++) {
            synthetic[header[i]] = firstRow[i]
          }
          sourceRecord = synthetic
        }
      }
      if (!sourceRecord) return
      for (const col of cols) {
        if (!isPasteable(col.def)) continue
        const sourceField = sourceFields.find((f) => f.name === col.name)
        const srcVal = sourceRecord[col.name]
        if (srcVal === undefined) continue
        const [next, didCoerce] = coerceValue(col.def.type, srcVal, sourceField?.type)
        if (didCoerce) coercedCount += 1
        edits.push({ recordId: rec.id, field: col.name, prev: rec[col.name] ?? null, next })
      }
    } else {
      // No selection — paste creates new records. Only a typed
      // `rows` payload or a TSV with a header row is supported here;
      // a bare cell matrix has nowhere to land without target
      // columns.
      if (read.payload?.kind === 'rows') {
        for (const rec of read.payload.records) {
          const seed: BaseRecord = { id: '' }
          for (const col of cols) {
            if (!isPasteable(col.def)) continue
            const sourceField = read.payload.fields.find((f) => f.name === col.name)
            const srcVal = rec[col.name]
            if (srcVal === undefined) continue
            const [next, didCoerce] = coerceValue(col.def.type, srcVal, sourceField?.type)
            if (didCoerce) coercedCount += 1
            seed[col.name] = next
          }
          created.push({ record: seed })
        }
      } else if (read.text) {
        const parsed = parseTsv(read.text)
        if (parsed.length < 2) return
        const header = parsed[0]
        for (let i = 1; i < parsed.length; i++) {
          const row = parsed[i]
          const seed: BaseRecord = { id: '' }
          for (let j = 0; j < header.length; j++) {
            const col = cols.find((c) => c.name === header[j])
            if (!col || !isPasteable(col.def)) continue
            const [next, didCoerce] = coerceValue(col.def.type, row[j], 'text')
            if (didCoerce) coercedCount += 1
            seed[col.name] = next
          }
          created.push({ record: seed })
        }
      }
    }

    if (edits.length === 0 && created.length === 0) return

    // Build a single HistoryEntry that captures both updated cells
    // and any newly-created records. Forward re-applies the edits +
    // re-creates the records (with the kernel-minted ids stable
    // across redo); inverse reverts cells to `prev` and deletes the
    // created rows.
    const createdIds: string[] = []
    const apply = async () => {
      const byRecord = new Map<string, Record<string, unknown>>()
      for (const e of edits) {
        const m = byRecord.get(e.recordId) ?? {}
        m[e.field] = e.next
        byRecord.set(e.recordId, m)
      }
      for (const [recordId, fields] of byRecord) {
        await client.updateRecord(relpath, recordId, fields)
        patchRecord(relpath, recordId, fields)
      }
      // Initial run: kernel mints ids for the created rows. Redo
      // re-uses those ids so subsequent history entries pinned to
      // them stay valid.
      const initial = createdIds.length === 0
      for (let i = 0; i < created.length; i++) {
        const seed = initial ? created[i].record : { ...created[i].record, id: createdIds[i] }
        const stored = await client.createRecord(relpath, seed)
        appendRecord(relpath, stored)
        if (initial) createdIds.push(stored.id)
      }
    }
    const revert = async () => {
      const byRecord = new Map<string, Record<string, unknown>>()
      for (const e of edits) {
        const m = byRecord.get(e.recordId) ?? {}
        m[e.field] = e.prev
        byRecord.set(e.recordId, m)
      }
      for (const [recordId, fields] of byRecord) {
        await client.updateRecord(relpath, recordId, fields)
        patchRecord(relpath, recordId, fields)
      }
      for (const id of createdIds) {
        await client.deleteRecord(relpath, id)
        removeRecord(relpath, id)
      }
    }
    try {
      setOpError(null)
      await apply()
      const label =
        created.length > 0
          ? `Paste ${created.length} row${created.length === 1 ? '' : 's'}`
          : edits.length === 1
            ? 'Paste cell'
            : `Paste ${edits.length} cells`
      pushHistory(relpath, { label, forward: apply, inverse: revert })
      if (coercedCount > 0) {
        const api = getBasesApi()
        api?.notifications.show({
          type: 'info',
          message: `bases:paste-coerced — ${coercedCount} cell${coercedCount === 1 ? '' : 's'} converted to fit destination types.`,
        })
      }
    } catch (err) {
      setOpError(`paste failed: ${errMsg(err)}`)
    }
  }, [
    appendRecord,
    client,
    patchRecord,
    pushHistory,
    readOnly,
    relpath,
    removeRecord,
  ])

  // Register the table-clipboard handle while this component is
  // mounted. The plugin's cut/copy/paste commands route through
  // tableClipboard; an unmounted table de-registers itself, so
  // palette invocations with no table focused are silent no-ops.
  useEffect(() => {
    setActiveTableClipboard({
      cut: () => void handleCut(),
      copy: () => void handleCopy(),
      paste: () => void handlePaste(),
    })
    return () => {
      setActiveTableClipboard(null)
    }
  }, [handleCut, handleCopy, handlePaste])

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
          borderBottom: '1px solid var(--background-modifier-border, #2a2a2e)',
          fontSize: 12,
          color: 'var(--text-muted, #9ca3af)',
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
                      background: 'var(--background-secondary, #252529)',
                      borderBottom: '1px solid var(--background-modifier-border, #2a2a2e)',
                      borderRight: '1px solid var(--background-modifier-border, #2a2a2e)',
                      color: 'var(--text-muted, #9ca3af)',
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
                        color: 'var(--text-faint, #6b7280)',
                      }}
                    >
                      {typeGlyph(c.def.type)}
                    </span>
                    <span style={{ color: 'var(--text-normal, #e4e4e7)' }}>{c.def.displayName ?? c.name}</span>
                    {arrow && (
                      <span style={{ marginLeft: 6, color: 'var(--interactive-accent, #60a5fa)' }}>
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
                        rowIndex={vr.index}
                        columns={columns}
                        selected={r.id === selectedRecordId}
                        cellSelection={cellSelection}
                        editing={editing?.id === r.id ? editing.field : null}
                        client={client}
                        rowHeight={ROW_HEIGHT}
                        onSelect={() => setSelectedRecordId(relpath, r.id)}
                        onCellClick={handleCellClick}
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
                    color: 'var(--text-muted, #9ca3af)',
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
  rowIndex: number
  columns: Column[]
  selected: boolean
  cellSelection: CellRange | null
  editing: string | null
  client: BasesKernelClient
  rowHeight: number
  onSelect(): void
  onCellClick(rowIndex: number, colIndex: number, shiftKey: boolean): void
  onStartEdit(field: string): void
  onCancelEdit(): void
  onCommit(field: string, value: unknown): void
}

function Row({
  record,
  rowIndex,
  columns,
  selected,
  cellSelection,
  editing,
  client,
  rowHeight,
  onSelect,
  onCellClick,
  onStartEdit,
  onCancelEdit,
  onCommit,
}: RowProps) {
  return (
    <tr
      onClick={onSelect}
      style={{
        background: selected ? 'var(--interactive-accent-soft, #2a2a35)' : 'transparent',
        cursor: 'default',
        height: rowHeight,
      }}
    >
      {columns.map((c, colIndex) => {
        const cellSelected =
          !!cellSelection && rangeContains(cellSelection, rowIndex, colIndex)
        return (
          <Cell
            key={c.name}
            field={c.name}
            def={c.def}
            value={record[c.name]}
            record={record}
            client={client}
            editing={editing === c.name}
            cellSelected={cellSelected}
            onCellClick={(shiftKey) => onCellClick(rowIndex, colIndex, shiftKey)}
            onStartEdit={() => onStartEdit(c.name)}
            onCancel={onCancelEdit}
            onCommit={(v) => onCommit(c.name, v)}
          />
        )
      })}
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
  cellSelected: boolean
  onCellClick(shiftKey: boolean): void
  onStartEdit(): void
  onCancel(): void
  onCommit(value: unknown): void
}

function Cell({
  field,
  def,
  value,
  record,
  client,
  editing,
  cellSelected,
  onCellClick,
  onStartEdit,
  onCancel,
  onCommit,
}: CellProps) {
  const readOnly = isReadOnly(def.type)
  const base: React.CSSProperties = {
    padding: '4px 10px',
    borderBottom: '1px solid var(--background-modifier-border, #2a2a2e)',
    borderRight: '1px solid var(--background-modifier-border, #2a2a2e)',
    verticalAlign: 'middle',
    color: readOnly ? 'var(--text-muted, #9ca3af)' : 'var(--text-normal, #e4e4e7)',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    // Cell-range selection lives over and above the row-selection
    // background — it's a stronger accent so the user can see the
    // exact rectangle while a row may also be highlighted.
    background: cellSelected ? 'var(--bg-cell-selected, rgba(96,165,250,0.18))' : undefined,
  }

  const handleClickCapture = (e: React.MouseEvent<HTMLTableCellElement>) => {
    // Suppress when the user is interacting with a checkbox / editor;
    // the click handlers there call `stopPropagation` already, but
    // we read e.target as a safety net.
    onCellClick(e.shiftKey)
  }

  // Checkbox toggles without an edit mode.
  if (def.type === 'checkbox' && !readOnly) {
    const checked = value === true
    return (
      <td style={base} onClick={handleClickCapture}>
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
      <td style={base} title={`formula · ${def.expression}`} onClick={handleClickCapture}>
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
      onClick={handleClickCapture}
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
        style={{ color: 'var(--interactive-accent, #60a5fa)' }}
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
              background: 'var(--background-secondary, #252529)',
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
    background: 'var(--background-primary, #1e1e22)',
    color: 'var(--text-normal, #e4e4e7)',
    border: '1px solid var(--interactive-accent, #60a5fa)',
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
  background: 'var(--background-secondary, #252529)',
  color: 'var(--text-normal, #e4e4e7)',
  border: '1px solid var(--background-modifier-border, #2a2a2e)',
  borderRadius: 3,
  fontSize: 11,
  cursor: 'pointer',
}
