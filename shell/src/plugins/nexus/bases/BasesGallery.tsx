// Phase 4 of docs/bases-shell-plan.md — Gallery view. Renders each
// record as a card; cover comes from a user-picked `url` field
// (PRD-10 has no `files` type yet, so we treat any URL ending in a
// common image extension as a cover, and otherwise render a solid
// placeholder). Cards select the record globally on click.

import { useMemo } from 'react'
import { useBasesStore } from './basesStore'
import type { Base, BaseRecord, BasesKernelClient } from './kernelClient'
import { formatValue, parseFieldDef, type FieldDefinition } from './fieldTypes'

interface Props {
  relpath: string
  base: Base
  /** Reserved — gallery is read-only today but will wire record
   *  mutations in Phase 6 (inline edits, add from gallery). */
  client: BasesKernelClient
}

interface Column {
  name: string
  def: FieldDefinition
}

const IMAGE_EXT = /\.(png|jpe?g|gif|webp|svg|avif|bmp)(\?.*)?$/i

export function BasesGallery({ relpath, base, client: _client }: Props) {
  const imageField = useBasesStore((s) => s.tabs[relpath]?.galleryImageField ?? null)
  const setImageField = useBasesStore((s) => s.setGalleryImageField)
  const setSelectedRecordId = useBasesStore((s) => s.setSelectedRecordId)

  const urlColumns = useMemo(
    () =>
      Object.entries(base.schema.fields ?? {})
        .map(([name, def]) => ({ name, def: parseFieldDef(def) }))
        .filter((c) => c.def.type === 'url'),
    [base],
  )
  const allColumns = useMemo(
    () =>
      Object.entries(base.schema.fields ?? {})
        .filter(([n]) => n !== 'id')
        .map(([name, def]) => ({ name, def: parseFieldDef(def) })),
    [base],
  )
  const primary = useMemo(
    () => allColumns.find((c) => c.def.primary) ?? allColumns[0],
    [allColumns],
  )
  const active = useMemo<Column | null>(() => {
    if (imageField) {
      const m = urlColumns.find((c) => c.name === imageField)
      if (m) return m
    }
    return urlColumns[0] ?? null
  }, [imageField, urlColumns])

  const detailColumns = useMemo(
    () => allColumns.filter((c) => c !== primary && c !== active).slice(0, 3),
    [allColumns, primary, active],
  )

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
        {urlColumns.length > 0 ? (
          <>
            <span>Cover</span>
            <select
              value={active?.name ?? ''}
              onChange={(e) => setImageField(relpath, e.target.value || null)}
              style={selectStyle}
            >
              {urlColumns.map((c) => (
                <option key={c.name} value={c.name}>
                  {c.name}
                </option>
              ))}
            </select>
          </>
        ) : (
          <span>
            No <code style={{ margin: '0 2px' }}>url</code> field — cards will render without an image.
          </span>
        )}
      </div>
      <div
        style={{
          flex: 1,
          overflow: 'auto',
          padding: 16,
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))',
          gap: 12,
          alignContent: 'start',
        }}
      >
        {base.records.map((r) => (
          <Card
            key={r.id}
            record={r}
            primary={primary}
            imageColumn={active}
            detail={detailColumns}
            onSelect={() => setSelectedRecordId(relpath, r.id)}
          />
        ))}
        {base.records.length === 0 && (
          <div style={{ gridColumn: '1/-1', color: 'var(--text-muted)', padding: 24 }}>
            No records yet.
          </div>
        )}
      </div>
    </div>
  )
}

function Card({
  record,
  primary,
  imageColumn,
  detail,
  onSelect,
}: {
  record: BaseRecord
  primary: Column | undefined
  imageColumn: Column | null
  detail: Column[]
  onSelect(): void
}) {
  const title = primary
    ? formatValue(primary.def.type, record[primary.name]) || 'Untitled'
    : record.id
  const rawImage = imageColumn ? record[imageColumn.name] : null
  const imageUrl =
    typeof rawImage === 'string' && IMAGE_EXT.test(rawImage) ? rawImage : null
  return (
    <button
      type="button"
      onClick={onSelect}
      style={{
        display: 'flex',
        flexDirection: 'column',
        background: 'var(--background-secondary)',
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 6,
        overflow: 'hidden',
        cursor: 'pointer',
        padding: 0,
        textAlign: 'left',
        color: 'var(--text-normal)',
      }}
    >
      <div
        style={{
          aspectRatio: '16 / 9',
          background: imageUrl
            ? `center/cover no-repeat url("${encodeURI(imageUrl)}")`
            : 'linear-gradient(135deg, var(--background-secondary), var(--background-secondary-alt))',
          borderBottom: '1px solid var(--background-modifier-border)',
        }}
      />
      <div style={{ padding: 10, display: 'flex', flexDirection: 'column', gap: 4 }}>
        <div style={{ fontWeight: 500, fontSize: 12 }}>{title}</div>
        {detail.map((c) => {
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
    </button>
  )
}

const selectStyle: React.CSSProperties = {
  background: 'var(--background-secondary)',
  color: 'var(--text-normal)',
  border: '1px solid var(--background-modifier-border)',
  borderRadius: 3,
  padding: '2px 6px',
  fontSize: 11,
}
