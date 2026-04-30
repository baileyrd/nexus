// CodeMirror 6 widget for inline `[[{db:query}]]` database-view blocks
// (BL-012 split 2 — PRD-08 §8.1).
//
// Decoration plumbing (split 3) walks the block tree, finds
// `BlockType::DatabaseView` blocks, and replaces their source range
// with a `Decoration.replace` carrying a `DatabaseViewWidget`. This
// module owns the widget itself plus the per-block fetch + render
// pipeline:
//
// 1. On `toDOM`, the widget calls `executeDatabaseView` via the
//    injected `EditorKernelClient` and inserts a placeholder.
// 2. The cache (`databaseViewCache`) ensures repeat selection moves
//    don't re-run the IPC for an unchanged block.
// 3. When the response lands, `renderApplied` builds a basic table /
//    grouped layout. Virtualization, undo wiring, and filter/sort
//    UX layer on top in splits 3 / 4 / 5.

import { WidgetType } from '@codemirror/view'

import type {
  AppliedGroup,
  AppliedLayout,
  AppliedRecord,
  DatabaseViewConfig,
  EditorKernelClient,
  ExecuteDatabaseViewResponse,
} from '../kernelClient'

/** Public dependencies the widget needs — injected so unit tests can
 *  swap in a mock kernel client without standing up the full editor. */
export interface DatabaseViewWidgetDeps {
  client: EditorKernelClient
  /** Optional override for the per-(path, config) memo cache. Defaults
   *  to the module-level singleton. */
  cache?: DatabaseViewCache
  /** Error sink. Defaults to `console.error`. */
  onError?: (message: string, err: unknown) => void
  /** Optional write-back callback for the BL-012 split-5 filter /
   *  sort UX. When wired, the widget renders an editable header
   *  with chips for the current filters / sorts and an "Add"
   *  affordance; each mutation calls `onUpdateConfig(newConfig)`,
   *  which the decoration extension uses to dispatch a CM
   *  transaction replacing the source range with the new
   *  `[[{db:…}]]` form. The markdown stays the truth and the
   *  next decoration rebuild parses the new spec like any other
   *  edit. */
  onUpdateConfig?: (newConfig: DatabaseViewConfig) => void
}

/**
 * CM6 block widget that resolves and renders a single inline
 * database-view block. Comparison key is `(databasePath, configHash)`
 * so an identical block stamps onto the same cache slot across
 * decoration rebuilds.
 */
export class DatabaseViewWidget extends WidgetType {
  private readonly key: string
  constructor(
    readonly databasePath: string,
    readonly viewConfig: DatabaseViewConfig,
    readonly deps: DatabaseViewWidgetDeps,
  ) {
    super()
    this.key = widgetKey(databasePath, viewConfig)
  }

  eq(other: DatabaseViewWidget): boolean {
    return this.key === other.key
  }

  toDOM(): HTMLElement {
    const wrap = document.createElement('div')
    wrap.className = 'cm-md-dbview-widget'
    wrap.setAttribute('data-database-path', this.databasePath)

    const cache = this.deps.cache ?? databaseViewCache
    const onError =
      this.deps.onError ??
      ((m, e) => {
        console.error(`[nexus.editor] ${m}:`, e)
      })

    if (this.deps.onUpdateConfig) {
      wrap.appendChild(
        renderHeader(this.viewConfig, this.deps.onUpdateConfig),
      )
    }

    const body = document.createElement('div')
    body.className = 'cm-md-dbview-body'
    wrap.appendChild(body)

    const cached = cache.peek(this.key)
    if (cached?.response) {
      body.replaceChildren(renderApplied(cached.response))
      return wrap
    }
    if (cached?.error) {
      body.replaceChildren(renderError(cached.error))
      return wrap
    }

    body.replaceChildren(renderPending())

    const promise = cache.run(this.key, () =>
      this.deps.client.executeDatabaseView(this.databasePath, this.viewConfig),
    )
    void promise.then(
      (resp) => {
        if (!body.isConnected) return
        body.replaceChildren(renderApplied(resp))
      },
      (err) => {
        if (!body.isConnected) return
        const error = err instanceof Error ? err : new Error(String(err))
        onError('execute_database_view failed', error)
        body.replaceChildren(renderError(error))
      },
    )

    return wrap
  }

  ignoreEvent(): boolean {
    // Pointer / keyboard events on the widget shouldn't fall through
    // to CM's selection logic — the embedded grid is its own
    // interaction surface. Selection inside the source range is still
    // reachable by clicking the surrounding line.
    return true
  }
}

// ── Cache ───────────────────────────────────────────────────────────────────

interface CacheEntry {
  promise?: Promise<ExecuteDatabaseViewResponse>
  response?: ExecuteDatabaseViewResponse
  error?: Error
}

/** Memo cache for database-view IPC results, keyed by widget key.
 *  Mirrors the structure of `fencedCodeRegistry`'s render cache —
 *  the widget consults this on every `toDOM` so re-renders triggered
 *  by selection moves don't re-run the full storage + database IPC
 *  pipeline. */
export class DatabaseViewCache {
  private readonly cache = new Map<string, CacheEntry>()
  private readonly limit: number

  constructor(limit = 32) {
    this.limit = limit
  }

  peek(key: string): { response?: ExecuteDatabaseViewResponse; error?: Error } | undefined {
    const hit = this.cache.get(key)
    if (!hit) return undefined
    return { response: hit.response, error: hit.error }
  }

  /** Start (or reuse) a fetch for `key`. Resolves with the response
   *  on success, rejects on error. The cached entry is updated in
   *  place when the promise settles so subsequent `peek` calls see
   *  the resolved value. */
  run(
    key: string,
    fetcher: () => Promise<ExecuteDatabaseViewResponse>,
  ): Promise<ExecuteDatabaseViewResponse> {
    const existing = this.cache.get(key)
    if (existing?.response) return Promise.resolve(existing.response)
    if (existing?.promise) return existing.promise

    const entry: CacheEntry = {}
    this.cache.set(key, entry)
    this.evictIfFull()

    const promise = fetcher().then(
      (resp) => {
        if (this.cache.get(key) !== entry) return resp
        entry.response = resp
        delete entry.promise
        return resp
      },
      (err) => {
        const error = err instanceof Error ? err : new Error(String(err))
        if (this.cache.get(key) !== entry) throw error
        entry.error = error
        delete entry.promise
        throw error
      },
    )
    entry.promise = promise
    return promise
  }

  /** Drop the cache slot for `key`. Used by split 4 (undo / external
   *  edit invalidation) once the change-event subscription lands. */
  invalidate(key: string): void {
    this.cache.delete(key)
  }

  /** Drop every cache slot whose key targets `databasePath`. Cache
   *  keys are `${databasePath} ${stable-config-JSON}` so a prefix
   *  match plus the trailing space terminator is sufficient. Returns
   *  the count of evicted entries — callers can use it to skip the
   *  decoration recompute when nothing was cached for the path. */
  invalidatePath(databasePath: string): number {
    const prefix = `${databasePath} `
    let n = 0
    for (const k of [...this.cache.keys()]) {
      if (k.startsWith(prefix)) {
        this.cache.delete(k)
        n++
      }
    }
    return n
  }

  /** Test helper — number of cache slots currently held. */
  size(): number {
    return this.cache.size
  }

  /** Drop everything. Used by tests; production callers prefer
   *  targeted `invalidate`. */
  clear(): void {
    this.cache.clear()
  }

  private evictIfFull(): void {
    while (this.cache.size > this.limit) {
      const oldest = this.cache.keys().next().value
      if (oldest === undefined) break
      this.cache.delete(oldest)
    }
  }
}

/** Module-level singleton used by widgets that don't override the
 *  cache via `DatabaseViewWidgetDeps`. */
export const databaseViewCache = new DatabaseViewCache()

// ── Rendering ───────────────────────────────────────────────────────────────

/** Build the editable filter / sort header. Renders chips for
 *  every active filter / sort with `×` removal buttons, plus an
 *  "Add filter" / "Add sort" affordance. Each mutation produces a
 *  fresh `DatabaseViewConfig` and invokes `onUpdate`; the
 *  decoration extension translates that into a CM transaction
 *  rewriting the inline `[[{db:…}]]` source. */
function renderHeader(
  config: DatabaseViewConfig,
  onUpdate: (next: DatabaseViewConfig) => void,
): HTMLElement {
  const header = document.createElement('div')
  header.className = 'cm-md-dbview-header'

  const summary = document.createElement('span')
  summary.className = 'cm-md-dbview-summary'
  summary.textContent = describeViewType(config.view_type)
  header.appendChild(summary)

  const filterRow = document.createElement('div')
  filterRow.className = 'cm-md-dbview-chip-row'
  filterRow.dataset.kind = 'filters'
  for (const [i, f] of config.filters.entries()) {
    filterRow.appendChild(
      makeChip(`filter: ${f}`, () => onUpdate(removeAt(config, 'filters', i))),
    )
  }
  filterRow.appendChild(
    makeAddForm('Add filter (e.g. "status = Done")', (raw) => {
      const trimmed = raw.trim()
      if (!trimmed) return
      onUpdate(append(config, 'filters', trimmed))
    }),
  )
  header.appendChild(filterRow)

  const sortRow = document.createElement('div')
  sortRow.className = 'cm-md-dbview-chip-row'
  sortRow.dataset.kind = 'sorts'
  for (const [i, s] of config.sorts.entries()) {
    sortRow.appendChild(
      makeChip(`sort: ${s}`, () => onUpdate(removeAt(config, 'sorts', i))),
    )
  }
  sortRow.appendChild(
    makeAddForm('Add sort (e.g. "due_date asc")', (raw) => {
      const trimmed = raw.trim()
      if (!trimmed) return
      onUpdate(append(config, 'sorts', trimmed))
    }),
  )
  header.appendChild(sortRow)

  return header
}

function describeViewType(view: DatabaseViewConfig['view_type']): string {
  switch (view.kind) {
    case 'table':
      return 'Table'
    case 'kanban':
      return `Kanban (group by: ${view.column_by})`
    case 'calendar':
      return `Calendar (date: ${view.date_field})`
    case 'gallery':
      return `Gallery (title: ${view.title_field})`
    case 'custom':
      return 'Custom view'
  }
}

function makeChip(label: string, onRemove: () => void): HTMLElement {
  const chip = document.createElement('span')
  chip.className = 'cm-md-dbview-chip'
  const text = document.createElement('span')
  text.className = 'cm-md-dbview-chip-text'
  text.textContent = label
  const x = document.createElement('button')
  x.type = 'button'
  x.className = 'cm-md-dbview-chip-remove'
  x.textContent = '×'
  x.title = `Remove ${label}`
  x.addEventListener('click', (ev) => {
    ev.stopPropagation()
    onRemove()
  })
  chip.append(text, x)
  return chip
}

function makeAddForm(
  placeholder: string,
  onSubmit: (raw: string) => void,
): HTMLElement {
  const form = document.createElement('form')
  form.className = 'cm-md-dbview-add'
  const input = document.createElement('input')
  input.type = 'text'
  input.placeholder = placeholder
  input.className = 'cm-md-dbview-add-input'
  const submit = document.createElement('button')
  submit.type = 'submit'
  submit.className = 'cm-md-dbview-add-submit'
  submit.textContent = '+'
  submit.title = placeholder
  form.append(input, submit)
  form.addEventListener('submit', (ev) => {
    ev.preventDefault()
    ev.stopPropagation()
    const raw = input.value
    input.value = ''
    onSubmit(raw)
  })
  return form
}

function append(
  config: DatabaseViewConfig,
  field: 'filters' | 'sorts' | 'hidden_columns',
  value: string,
): DatabaseViewConfig {
  return { ...config, [field]: [...config[field], value] }
}

function removeAt(
  config: DatabaseViewConfig,
  field: 'filters' | 'sorts' | 'hidden_columns',
  index: number,
): DatabaseViewConfig {
  const next = [...config[field]]
  next.splice(index, 1)
  return { ...config, [field]: next }
}

/** Internal: build the resolved-state DOM for an `AppliedView`. */
function renderApplied(resp: ExecuteDatabaseViewResponse): HTMLElement {
  const layout = resp.applied.layout
  const fields = effectiveFields(resp)
  switch (layout.kind) {
    case 'flat':
      return renderFlat(fields, layout.records)
    case 'grouped':
      return renderGrouped(fields, layout.groups)
  }
}

function renderPending(): HTMLElement {
  const el = document.createElement('div')
  el.className = 'cm-md-dbview-pending'
  el.textContent = 'Loading…'
  return el
}

function renderError(err: Error): HTMLElement {
  const box = document.createElement('div')
  box.className = 'cm-md-dbview-error'
  const tag = document.createElement('span')
  tag.className = 'cm-md-dbview-error-tag'
  tag.textContent = 'database view'
  const msg = document.createElement('span')
  msg.className = 'cm-md-dbview-error-msg'
  msg.textContent = err.message || 'failed to render'
  box.append(tag, msg)
  return box
}

function renderFlat(fields: string[], records: AppliedRecord[]): HTMLElement {
  const table = document.createElement('table')
  table.className = 'cm-md-dbview-table'
  table.appendChild(buildHeader(fields))
  const tbody = document.createElement('tbody')
  for (const record of records) {
    tbody.appendChild(buildRow(fields, record))
  }
  if (records.length === 0) {
    const empty = document.createElement('div')
    empty.className = 'cm-md-dbview-empty'
    empty.textContent = 'No records.'
    const wrap = document.createElement('div')
    wrap.append(table, empty)
    return wrap
  }
  table.appendChild(tbody)
  return table
}

function renderGrouped(fields: string[], groups: AppliedGroup[]): HTMLElement {
  const wrap = document.createElement('div')
  wrap.className = 'cm-md-dbview-grouped'
  for (const group of groups) {
    const section = document.createElement('section')
    section.className = 'cm-md-dbview-group'
    const heading = document.createElement('h4')
    heading.className = 'cm-md-dbview-group-heading'
    heading.textContent = `${group.key} (${group.records.length})`
    section.appendChild(heading)
    const table = document.createElement('table')
    table.className = 'cm-md-dbview-table'
    table.appendChild(buildHeader(fields))
    const tbody = document.createElement('tbody')
    for (const record of group.records) {
      tbody.appendChild(buildRow(fields, record))
    }
    table.appendChild(tbody)
    section.appendChild(table)
    wrap.appendChild(section)
  }
  return wrap
}

function buildHeader(fields: string[]): HTMLElement {
  const thead = document.createElement('thead')
  const row = document.createElement('tr')
  for (const f of fields) {
    const th = document.createElement('th')
    th.textContent = f
    row.appendChild(th)
  }
  thead.appendChild(row)
  return thead
}

function buildRow(fields: string[], record: AppliedRecord): HTMLElement {
  const tr = document.createElement('tr')
  tr.dataset.recordId = record.id
  for (const f of fields) {
    const td = document.createElement('td')
    const value = record[f]
    td.textContent = formatCell(value)
    tr.appendChild(td)
  }
  return tr
}

/** Field list for the rendered grid. The view's explicit `fields`
 *  list wins; absent that, derive a stable order from the schema
 *  (insertion order in the underlying `serde_json::Map`); absent
 *  even that, fall back to whatever keys the first record carries.
 *  Matches the CLI / TUI base-renderer convention so the same query
 *  produces the same column set in every frontend. */
export function effectiveFields(resp: ExecuteDatabaseViewResponse): string[] {
  if (resp.applied.fields.length > 0) return resp.applied.fields
  const schemaFields = Object.keys(resp.schema.fields ?? {})
  if (schemaFields.length > 0) return schemaFields
  const layout: AppliedLayout = resp.applied.layout
  const first =
    layout.kind === 'flat'
      ? layout.records[0]
      : layout.groups.find((g) => g.records.length > 0)?.records[0]
  if (!first) return []
  return Object.keys(first).filter((k) => k !== 'id' && k !== 'deletedAt')
}

function formatCell(value: unknown): string {
  if (value === null || value === undefined) return ''
  if (typeof value === 'string') return value
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  // Arrays / objects — JSON, but bounded so a large list doesn't
  // blow up the cell. Real type-aware formatting lands in split 5.
  try {
    const json = JSON.stringify(value)
    return json.length > 200 ? `${json.slice(0, 197)}…` : json
  } catch {
    return ''
  }
}

/** Stable comparison key for a (path, config) pair. Pure-function;
 *  exported so tests and split-3 decoration plumbing can reuse it. */
export function widgetKey(databasePath: string, config: DatabaseViewConfig): string {
  return `${databasePath} ${stableConfigJson(config)}`
}

/** Deterministic JSON encoding of the config — sorts each object's
 *  keys so two structurally-equal configs hash identically
 *  regardless of insertion order. `JSON.stringify` alone would
 *  cache-miss on transient key reordering from the kernel side. */
function stableConfigJson(config: DatabaseViewConfig): string {
  return JSON.stringify(config, (_key, value) => {
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      const obj = value as Record<string, unknown>
      const sorted: Record<string, unknown> = {}
      for (const k of Object.keys(obj).sort()) sorted[k] = obj[k]
      return sorted
    }
    return value
  })
}
