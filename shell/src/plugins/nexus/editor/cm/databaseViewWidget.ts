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
import { formatCell, lookupFieldDef, type FieldDef } from './databaseViewFormat'

/** Mutation handle threaded into layout renderers that support
 *  write-back (kanban drag-to-reorder is the first). `update`
 *  patches one record's fields via `com.nexus.storage::base_record_update`;
 *  `refresh` invalidates the widget's cached layout and replays the
 *  fetch so the freshly-mutated record renders in its new bucket.
 *  Renderers without write-back ignore this argument. */
export interface MutationDeps {
  update: (
    recordId: string,
    fields: Record<string, unknown>,
  ) => Promise<void>
  refresh: () => void
}

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
      body.replaceChildren(renderApplied(cached.response, this.viewConfig))
      return wrap
    }
    if (cached?.error) {
      body.replaceChildren(renderError(cached.error))
      return wrap
    }

    body.replaceChildren(renderPending())

    // Mutation surface threaded into renderApplied so layouts that
    // support write-back (kanban drag-to-reorder is the first) can
    // update a record and trigger a re-fetch without the renderer
    // knowing about IPC plumbing or the cache. `refresh` is a thin
    // closure over the same fetch path the initial render uses.
    const databasePath = this.databasePath
    const viewConfig = this.viewConfig
    const client = this.deps.client
    const refresh = () => {
      cache.invalidate(this.key)
      if (!body.isConnected) return
      body.replaceChildren(renderPending())
      const p = cache.run(this.key, () =>
        client.executeDatabaseView(databasePath, viewConfig),
      )
      void p.then(
        (resp) => {
          if (!body.isConnected) return
          body.replaceChildren(renderApplied(resp, viewConfig, mutate))
        },
        (err) => {
          if (!body.isConnected) return
          const error = err instanceof Error ? err : new Error(String(err))
          onError('execute_database_view failed', error)
          body.replaceChildren(renderError(error))
        },
      )
    }
    const mutate: MutationDeps = {
      update: async (recordId, fields) => {
        await client.updateBaseRecord(databasePath, recordId, fields)
      },
      refresh,
    }

    const promise = cache.run(this.key, () =>
      client.executeDatabaseView(databasePath, viewConfig),
    )
    void promise.then(
      (resp) => {
        if (!body.isConnected) return
        body.replaceChildren(renderApplied(resp, viewConfig, mutate))
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

/** Internal: build the resolved-state DOM for an `AppliedView`.
 *  Dispatches on `applied.view_type` first so Kanban / Calendar /
 *  Gallery get layout-specific renderers; falls back to the
 *  layout-shape switch when the view type is one we don't have a
 *  dedicated renderer for (List, Timeline, Custom). The original
 *  `viewConfig` is threaded through so layout-specific metadata
 *  (`column_by`, `date_field`, `title_field`) survives the trip
 *  through `apply_view`'s minimal `BaseView` projection. */
function renderApplied(
  resp: ExecuteDatabaseViewResponse,
  viewConfig: DatabaseViewConfig,
  mutate?: MutationDeps,
): HTMLElement {
  const layout = resp.applied.layout
  const fields = effectiveFields(resp)
  const schema = resp.schema.fields
  switch (resp.applied.view_type) {
    case 'kanban':
      return layout.kind === 'grouped'
        ? renderKanban(fields, layout.groups, schema, viewConfig, mutate)
        : renderFlat(fields, layout.records, schema, mutate)
    case 'calendar':
      return layout.kind === 'grouped'
        ? renderCalendar(fields, layout.groups, schema, viewConfig)
        : renderFlat(fields, layout.records, schema, mutate)
    case 'gallery':
      return layout.kind === 'flat'
        ? renderGallery(fields, layout.records, schema, viewConfig, mutate)
        : renderGrouped(fields, layout.groups, schema, mutate)
    case 'table':
    case 'list':
    case 'timeline':
    default:
      return layout.kind === 'flat'
        ? renderFlat(fields, layout.records, schema, mutate)
        : renderGrouped(fields, layout.groups, schema, mutate)
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

function renderFlat(
  fields: string[],
  records: AppliedRecord[],
  schema: Record<string, unknown>,
  mutate?: MutationDeps,
): HTMLElement {
  const table = document.createElement('table')
  table.className = 'cm-md-dbview-table'
  table.appendChild(buildHeader(fields))
  const tbody = document.createElement('tbody')
  for (const record of records) {
    tbody.appendChild(buildRow(fields, record, schema, mutate))
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

function renderGrouped(
  fields: string[],
  groups: AppliedGroup[],
  schema: Record<string, unknown>,
  mutate?: MutationDeps,
): HTMLElement {
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
      tbody.appendChild(buildRow(fields, record, schema, mutate))
    }
    table.appendChild(tbody)
    section.appendChild(table)
    wrap.appendChild(section)
  }
  return wrap
}

/** Kanban — horizontal row of columns, one per group key. Each
 *  column has a heading (group value + count) and a vertical stack
 *  of compact cards. The card title is derived from the first
 *  text-typed field that isn't the group_by field; remaining
 *  visible fields render as labeled rows. Drag-to-reorder + write
 *  back is a deferred BL-069 follow-up. */
export function renderKanban(
  fields: string[],
  groups: AppliedGroup[],
  schema: Record<string, unknown>,
  viewConfig: DatabaseViewConfig,
  mutate?: MutationDeps,
): HTMLElement {
  const board = document.createElement('div')
  board.className = 'cm-md-dbview-kanban'
  const groupField =
    viewConfig.view_type.kind === 'kanban'
      ? viewConfig.view_type.column_by
      : viewConfig.group_by ?? null
  const cardFields = fields.filter((f) => f !== groupField)
  // Drag-to-reorder is gated on having both a mutation surface and
  // a known group field — without `groupField` the writer wouldn't
  // know which key to patch, and without `mutate` there's nowhere
  // to write to. The renderer falls back to static cards in either
  // case (the legacy pre-BL-069-tail behaviour).
  const dndEnabled = mutate != null && groupField != null
  for (const group of groups) {
    const column = document.createElement('section')
    column.className = 'cm-md-dbview-kanban-column'
    column.dataset.groupKey = group.key
    const heading = document.createElement('header')
    heading.className = 'cm-md-dbview-kanban-heading'
    const label = document.createElement('span')
    label.className = 'cm-md-dbview-kanban-label'
    label.textContent = group.key
    const count = document.createElement('span')
    count.className = 'cm-md-dbview-kanban-count'
    count.textContent = String(group.records.length)
    heading.append(label, count)
    column.appendChild(heading)
    for (const record of group.records) {
      const card = buildCard(record, cardFields, schema, /*titleField*/ null, mutate)
      if (dndEnabled) {
        card.draggable = true
        card.dataset.recordId = record.id
        card.dataset.sourceGroupKey = group.key
        card.classList.add('cm-md-dbview-card--draggable')
        card.addEventListener('dragstart', (ev) => {
          if (!ev.dataTransfer) return
          ev.dataTransfer.effectAllowed = 'move'
          // Encode payload so cross-board drags (multiple kanbans
          // on the same page) can't accidentally swap records.
          ev.dataTransfer.setData(
            KANBAN_DRAG_MIME,
            JSON.stringify({
              record_id: record.id,
              source_group_key: group.key,
            }),
          )
          card.classList.add('cm-md-dbview-card--dragging')
        })
        card.addEventListener('dragend', () => {
          card.classList.remove('cm-md-dbview-card--dragging')
        })
      }
      column.appendChild(card)
    }
    if (group.records.length === 0) {
      const empty = document.createElement('div')
      empty.className = 'cm-md-dbview-kanban-empty'
      empty.textContent = '—'
      column.appendChild(empty)
    }
    if (dndEnabled) {
      column.addEventListener('dragover', (ev) => {
        // Default behaviour rejects drops; preventing it signals we
        // accept the drag. effectAllowed/dropEffect keep the cursor
        // honest about what's about to happen.
        if (!ev.dataTransfer) return
        if (!ev.dataTransfer.types.includes(KANBAN_DRAG_MIME)) return
        ev.preventDefault()
        ev.dataTransfer.dropEffect = 'move'
        column.classList.add('cm-md-dbview-kanban-column--drop-target')
      })
      column.addEventListener('dragleave', () => {
        column.classList.remove('cm-md-dbview-kanban-column--drop-target')
      })
      column.addEventListener('drop', (ev) => {
        column.classList.remove('cm-md-dbview-kanban-column--drop-target')
        if (!ev.dataTransfer) return
        const payload = readKanbanDragPayload(
          ev.dataTransfer.getData(KANBAN_DRAG_MIME),
        )
        if (!payload) return
        ev.preventDefault()
        // Fire-and-forget the IPC; on success refresh re-fetches
        // and the new layout replaces this DOM. On failure the
        // renderer leaves the DOM unchanged and surfaces via a
        // console warning — a toast surface lands when BL-069's
        // mutation UI grows past kanban.
        void applyKanbanDrop(payload, group.key, groupField as string, mutate!).catch(
          (err) => {
            console.warn('[nexus.editor] kanban drop update failed:', err)
          },
        )
      })
    }
    board.appendChild(column)
  }
  return board
}

/** Custom MIME type for the kanban drag payload. Picking a Nexus-
 *  specific type keeps the widget's drop logic from accepting
 *  arbitrary text drags from outside the board. */
export const KANBAN_DRAG_MIME = 'application/x-nexus-dbview-kanban'

/** Sentinel string the database engine uses for records that have
 *  no value in the group_by field. Mirrors `MISSING_GROUP_KEY` in
 *  `nexus-database`; surfaces as the "(none)" / "Undated" bucket
 *  in the rendered layouts. We map it to a JSON `null` on
 *  drop-back-to-none so the storage layer clears the field rather
 *  than writing the literal string `"(none)"`. */
export const MISSING_GROUP_KEY = '(none)'

/** Payload exchanged through `dataTransfer` for a kanban card drag. */
export interface KanbanDragPayload {
  record_id: string
  source_group_key: string
}

/** Parse a drop event's transfer payload. Returns `null` for
 *  unrelated MIMEs, malformed JSON, or missing fields — the caller
 *  treats `null` as "no-op" so a misfired drop never panics the
 *  widget. */
export function readKanbanDragPayload(raw: string | null | undefined): KanbanDragPayload | null {
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as Partial<KanbanDragPayload>
    if (typeof parsed.record_id !== 'string') return null
    if (typeof parsed.source_group_key !== 'string') return null
    return { record_id: parsed.record_id, source_group_key: parsed.source_group_key }
  } catch {
    return null
  }
}

/** Apply a kanban drop: if the card actually moved between groups,
 *  patch the record's `group_field` value and refresh. The
 *  "moved-to-the-undated-bucket" case maps the `MISSING_GROUP_KEY`
 *  sentinel to a JSON `null` so the storage layer clears the
 *  field rather than writing the literal `"(none)"` string.
 *
 *  Returns the mutation result for tests / callers that want to
 *  observe completion: `{ updated: true }` after a successful
 *  patch + refresh, `{ updated: false }` when the drop landed in
 *  the same column (early-return). Errors propagate so the
 *  caller decides whether to surface them. */
export async function applyKanbanDrop(
  payload: KanbanDragPayload,
  destinationGroupKey: string,
  groupField: string,
  mutate: MutationDeps,
): Promise<{ updated: boolean }> {
  if (payload.source_group_key === destinationGroupKey) {
    return { updated: false }
  }
  const nextValue: unknown =
    destinationGroupKey === MISSING_GROUP_KEY ? null : destinationGroupKey
  await mutate.update(payload.record_id, { [groupField]: nextValue })
  mutate.refresh()
  return { updated: true }
}

/** Calendar — month grid (7×N rows) bucketing groups by ISO date.
 *  The visible month is derived from the median date in the data so
 *  the user lands on the densest area without a navigation control
 *  (deferred). Records appear as compact pill buttons inside each
 *  day cell; an "undated" bucket sits below the grid for `(none)`
 *  groups (the `MISSING_GROUP_KEY` sentinel from the database
 *  engine). */
export function renderCalendar(
  fields: string[],
  groups: AppliedGroup[],
  schema: Record<string, unknown>,
  viewConfig: DatabaseViewConfig,
): HTMLElement {
  void schema // unused — date pills don't render full cells
  void fields
  const wrap = document.createElement('div')
  wrap.className = 'cm-md-dbview-calendar'

  const dateField =
    viewConfig.view_type.kind === 'calendar'
      ? viewConfig.view_type.date_field
      : null
  const dated: Array<{ date: Date; group: AppliedGroup }> = []
  const undated: AppliedGroup[] = []
  for (const g of groups) {
    const d = parseIsoDate(g.key)
    if (d) dated.push({ date: d, group: g })
    else undated.push(g)
  }

  if (dated.length === 0 && undated.length === 0) {
    wrap.appendChild(emptyState('No records.'))
    return wrap
  }

  // Pick the visible month from the median date so the grid lands
  // on the densest area regardless of outliers.
  const sortedDates = [...dated.map((d) => d.date)].sort(
    (a, b) => a.getTime() - b.getTime(),
  )
  const visibleAnchor =
    sortedDates.length > 0 ? sortedDates[Math.floor(sortedDates.length / 2)] : new Date()
  const year = visibleAnchor.getUTCFullYear()
  const month = visibleAnchor.getUTCMonth()

  const monthLabel = document.createElement('header')
  monthLabel.className = 'cm-md-dbview-calendar-month'
  monthLabel.textContent = `${monthName(month)} ${year}${
    dateField ? ` · ${dateField}` : ''
  }`
  wrap.appendChild(monthLabel)

  const grid = document.createElement('div')
  grid.className = 'cm-md-dbview-calendar-grid'
  for (const dow of ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']) {
    const head = document.createElement('div')
    head.className = 'cm-md-dbview-calendar-dow'
    head.textContent = dow
    grid.appendChild(head)
  }

  // Build a (year-month-day) → group map for fast cell lookup.
  const byKey = new Map<string, AppliedGroup>()
  for (const { date, group } of dated) {
    byKey.set(toIsoYmd(date), group)
  }

  // Anchor the grid at the Sunday on or before the 1st of the
  // visible month, and run for 6 weeks (42 cells) so months that
  // wrap stay covered.
  const firstOfMonth = new Date(Date.UTC(year, month, 1))
  const startOffset = firstOfMonth.getUTCDay()
  const gridStart = new Date(
    Date.UTC(year, month, 1 - startOffset),
  )
  for (let i = 0; i < 42; i++) {
    const cellDate = new Date(
      Date.UTC(
        gridStart.getUTCFullYear(),
        gridStart.getUTCMonth(),
        gridStart.getUTCDate() + i,
      ),
    )
    const cell = document.createElement('div')
    cell.className = 'cm-md-dbview-calendar-cell'
    if (cellDate.getUTCMonth() !== month) {
      cell.classList.add('cm-md-dbview-calendar-cell--other-month')
    }
    const num = document.createElement('span')
    num.className = 'cm-md-dbview-calendar-day'
    num.textContent = String(cellDate.getUTCDate())
    cell.appendChild(num)
    const ymd = toIsoYmd(cellDate)
    const group = byKey.get(ymd)
    if (group) {
      for (const record of group.records) {
        const pill = document.createElement('div')
        pill.className = 'cm-md-dbview-calendar-pill'
        pill.dataset.recordId = record.id
        pill.textContent = recordTitle(record, schema, /*titleField*/ null)
        cell.appendChild(pill)
      }
    }
    grid.appendChild(cell)
  }
  wrap.appendChild(grid)

  if (undated.length > 0) {
    const undatedSection = document.createElement('section')
    undatedSection.className = 'cm-md-dbview-calendar-undated'
    const heading = document.createElement('h4')
    heading.textContent = 'Undated'
    undatedSection.appendChild(heading)
    for (const g of undated) {
      for (const record of g.records) {
        const pill = document.createElement('div')
        pill.className = 'cm-md-dbview-calendar-pill cm-md-dbview-calendar-pill--undated'
        pill.dataset.recordId = record.id
        pill.textContent = recordTitle(record, schema, /*titleField*/ null)
        undatedSection.appendChild(pill)
      }
    }
    wrap.appendChild(undatedSection)
  }

  return wrap
}

/** Gallery — flat record list rendered as cards. Uses the
 *  configured `title_field` when present (Gallery view stores it on
 *  the view-type variant); otherwise the first non-id text field
 *  wins, matching the kanban / calendar fallback. Body fields are
 *  capped at 5 per card to keep the gallery scannable. */
export function renderGallery(
  fields: string[],
  records: AppliedRecord[],
  schema: Record<string, unknown>,
  viewConfig: DatabaseViewConfig,
  mutate?: MutationDeps,
): HTMLElement {
  const grid = document.createElement('div')
  grid.className = 'cm-md-dbview-gallery'
  const titleField =
    viewConfig.view_type.kind === 'gallery'
      ? viewConfig.view_type.title_field
      : null
  if (records.length === 0) {
    grid.appendChild(emptyState('No records.'))
    return grid
  }
  const bodyFields = fields.filter((f) => f !== titleField)
  for (const record of records) {
    grid.appendChild(buildCard(record, bodyFields, schema, titleField, mutate))
  }
  return grid
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

function buildRow(
  fields: string[],
  record: AppliedRecord,
  schema: Record<string, unknown>,
  mutate?: MutationDeps,
): HTMLElement {
  const tr = document.createElement('tr')
  tr.dataset.recordId = record.id
  for (const f of fields) {
    const td = document.createElement('td')
    const value = record[f]
    const def = lookupFieldDef(schema, f)
    if (mutate && isEditableType(def?.type)) {
      td.appendChild(makeEditableCell(record, f, value, def, mutate))
    } else {
      td.textContent = formatCell(value, def)
    }
    tr.appendChild(td)
  }
  return tr
}

/** Build a single record card used by Kanban + Gallery. The card's
 *  title comes from `titleField` when it points at a present field,
 *  otherwise the first non-empty text field from `bodyFields` wins,
 *  otherwise the record id. Body shows up to 5 labeled rows. */
function buildCard(
  record: AppliedRecord,
  bodyFields: string[],
  schema: Record<string, unknown>,
  titleField: string | null,
  mutate?: MutationDeps,
): HTMLElement {
  const card = document.createElement('article')
  card.className = 'cm-md-dbview-card'
  card.dataset.recordId = record.id

  const title = document.createElement('header')
  title.className = 'cm-md-dbview-card-title'
  title.textContent = recordTitle(record, schema, titleField)
  card.appendChild(title)

  const body = document.createElement('div')
  body.className = 'cm-md-dbview-card-body'
  const remaining = titleField
    ? bodyFields.filter((f) => f !== titleField)
    : bodyFields
  let shown = 0
  const MAX_BODY_FIELDS = 5
  for (const f of remaining) {
    if (shown >= MAX_BODY_FIELDS) break
    const value = record[f]
    if (value === null || value === undefined) continue
    const def = lookupFieldDef(schema, f)
    const formatted = formatCell(value, def)
    if (formatted === '') continue
    const row = document.createElement('div')
    row.className = 'cm-md-dbview-card-row'
    const label = document.createElement('span')
    label.className = 'cm-md-dbview-card-label'
    label.textContent = f
    if (mutate && isEditableType(def?.type)) {
      const editable = makeEditableCell(record, f, value, def, mutate)
      editable.classList.add('cm-md-dbview-card-value')
      row.append(label, editable)
    } else {
      const v = document.createElement('span')
      v.className = 'cm-md-dbview-card-value'
      v.textContent = formatted
      row.append(label, v)
    }
    body.appendChild(row)
    shown += 1
  }
  card.appendChild(body)
  return card
}

// ── Inline cell editing (BL-069 follow-up) ──────────────────────────────────

/** Field types that the inline editor knows how to edit. Anything
 *  outside this set renders read-only — an undefined `def.type` also
 *  routes here so legacy schemas without type metadata don't pop a
 *  text editor over what could be structured data. The list mirrors
 *  the editable subset of `nexus_types::bases::FieldType`. */
const EDITABLE_TYPES = new Set([
  'text',
  'long-text',
  'url',
  'email',
  'uuid',
  'number',
  'currency',
  'percent',
  'checkbox',
  'date',
  'time',
  'datetime',
  'select',
])

/** Whether a field with this `def.type` should render as an editable
 *  cell. Exported so the test suite can pin the exact set without
 *  duplicating the list. */
export function isEditableType(type: string | undefined): boolean {
  if (!type) return false
  return EDITABLE_TYPES.has(type)
}

/** Render the click-to-edit cell. The default state is a `<span>`
 *  carrying the formatted text; a click swaps it for a type-aware
 *  editor (input / select / etc). Commit (Enter / blur / change for
 *  selects + checkbox) calls `mutate.update` then `mutate.refresh`;
 *  Escape restores the read-only display without firing the IPC.
 *
 *  The cell registers a `data-field` attribute on the wrapper so the
 *  test harness can target a specific cell without having to walk
 *  the DOM by index. */
export function makeEditableCell(
  record: AppliedRecord,
  field: string,
  value: unknown,
  def: FieldDef | undefined,
  mutate: MutationDeps,
): HTMLElement {
  const wrap = document.createElement('span')
  wrap.className = 'cm-md-dbview-cell'
  wrap.dataset.field = field
  wrap.dataset.recordId = record.id

  // Checkbox is special: no popover, click toggles directly. The
  // formatted glyph (✓ / empty) doubles as the affordance.
  if (def?.type === 'checkbox') {
    renderCheckbox(wrap, record, field, value === true, mutate)
    return wrap
  }

  const display = document.createElement('span')
  display.className = 'cm-md-dbview-cell-display'
  display.textContent = formatCell(value, def)
  wrap.appendChild(display)
  wrap.classList.add('cm-md-dbview-cell--editable')
  wrap.title = 'Click to edit'

  display.addEventListener('click', (ev) => {
    ev.stopPropagation()
    swapToEditor(wrap, display, record, field, value, def, mutate)
  })

  return wrap
}

function renderCheckbox(
  wrap: HTMLElement,
  record: AppliedRecord,
  field: string,
  checked: boolean,
  mutate: MutationDeps,
): void {
  wrap.classList.add('cm-md-dbview-cell--checkbox')
  wrap.classList.add('cm-md-dbview-cell--editable')
  wrap.title = 'Click to toggle'
  const glyph = document.createElement('span')
  glyph.className = 'cm-md-dbview-cell-display'
  glyph.textContent = checked ? '✓' : ''
  wrap.appendChild(glyph)
  wrap.addEventListener('click', (ev) => {
    ev.stopPropagation()
    void commitCellMutation(wrap, record.id, field, !checked, mutate)
  })
}

function swapToEditor(
  wrap: HTMLElement,
  display: HTMLElement,
  record: AppliedRecord,
  field: string,
  value: unknown,
  def: FieldDef | undefined,
  mutate: MutationDeps,
): void {
  const editor = buildEditorControl(value, def)
  if (!editor) return
  wrap.classList.add('cm-md-dbview-cell--editing')
  display.replaceWith(editor)
  if (editor instanceof HTMLInputElement || editor instanceof HTMLSelectElement) {
    editor.focus()
    if (editor instanceof HTMLInputElement && typeof editor.select === 'function') {
      editor.select()
    }
  }

  let settled = false
  const restore = (): void => {
    if (settled) return
    settled = true
    wrap.classList.remove('cm-md-dbview-cell--editing')
    editor.replaceWith(display)
  }
  const commit = (raw: string): void => {
    if (settled) return
    const parsed = parseCellInput(raw, def)
    if (parsed === SAME_VALUE) {
      restore()
      return
    }
    // Skip the IPC when the parsed value matches what's already on
    // the record — Enter / blur on an unchanged input is a no-op.
    if (parsed === value || (parsed === null && value == null)) {
      restore()
      return
    }
    settled = true
    wrap.classList.remove('cm-md-dbview-cell--editing')
    void commitCellMutation(wrap, record.id, field, parsed, mutate).finally(() => {
      // refresh() rebuilds the layout from scratch on success, so
      // this DOM is about to be replaced. On failure we restore the
      // pre-edit display so the user sees what was there before.
      if (editor.isConnected) editor.replaceWith(display)
    })
  }

  editor.addEventListener('keydown', (ev: Event) => {
    const ke = ev as KeyboardEvent
    if (ke.key === 'Enter') {
      ev.preventDefault()
      commit(readEditorValue(editor))
    } else if (ke.key === 'Escape') {
      ev.preventDefault()
      restore()
    }
  })
  editor.addEventListener('blur', () => {
    commit(readEditorValue(editor))
  })
  // <select> commits on change so a single click → pick → commit
  // works without leaving the cell focused.
  if (editor instanceof HTMLSelectElement) {
    editor.addEventListener('change', () => {
      commit(readEditorValue(editor))
    })
  }
}

function buildEditorControl(
  value: unknown,
  def: FieldDef | undefined,
): HTMLInputElement | HTMLSelectElement | null {
  const type = def?.type
  if (type === 'select') {
    const select = document.createElement('select')
    select.className = 'cm-md-dbview-cell-editor'
    // Render an explicit empty option so the user can clear the field.
    const empty = document.createElement('option')
    empty.value = ''
    empty.textContent = '—'
    select.appendChild(empty)
    const current = formatCell(value, def)
    for (const opt of def?.options ?? []) {
      const optionEl = document.createElement('option')
      const label = typeof opt === 'string' ? opt : (opt.label ?? opt.name ?? opt.id ?? '')
      const v = typeof opt === 'string' ? opt : (opt.id ?? opt.label ?? opt.name ?? '')
      optionEl.value = String(v)
      optionEl.textContent = String(label)
      select.appendChild(optionEl)
    }
    select.value = current
    return select
  }
  const input = document.createElement('input')
  input.className = 'cm-md-dbview-cell-editor'
  switch (type) {
    case 'number':
    case 'currency':
    case 'percent':
      input.type = 'number'
      input.value = value == null ? '' : String(value)
      break
    case 'date':
      input.type = 'date'
      input.value = formatCell(value, def)
      break
    case 'time':
      input.type = 'time'
      input.value = formatCell(value, def)
      break
    case 'datetime':
      input.type = 'datetime-local'
      // <input type=datetime-local> wants `YYYY-MM-DDTHH:MM`, not the
      // formatter's space-separated form.
      input.value = formatCell(value, def).replace(' ', 'T')
      break
    default:
      input.type = 'text'
      input.value = typeof value === 'string' ? value : formatCell(value, def)
      break
  }
  return input
}

function readEditorValue(editor: HTMLInputElement | HTMLSelectElement): string {
  return editor.value
}

/** Sentinel returned by `parseCellInput` when the parsed value is
 *  identical to the original — the caller short-circuits the IPC and
 *  just restores the display. */
const SAME_VALUE = Symbol('SAME_VALUE')

/** Parse the raw string from the editor control into the wire-shape
 *  value that `update_record` expects. Empty / whitespace-only inputs
 *  collapse to JSON null so the storage layer clears the field
 *  rather than writing the literal empty string. Numbers parse via
 *  `Number()`; non-finite results bail out (caller treats `NaN` as a
 *  no-op via the SAME_VALUE sentinel). Exported for unit tests. */
export function parseCellInput(
  raw: string,
  def: FieldDef | undefined,
): unknown {
  const trimmed = raw.trim()
  if (trimmed === '') return null
  switch (def?.type) {
    case 'number':
    case 'currency':
    case 'percent': {
      const n = Number(trimmed)
      if (!Number.isFinite(n)) return SAME_VALUE
      return n
    }
    default:
      return trimmed
  }
}

async function commitCellMutation(
  wrap: HTMLElement,
  recordId: string,
  field: string,
  parsed: unknown,
  mutate: MutationDeps,
): Promise<void> {
  if (parsed === SAME_VALUE) return
  wrap.classList.add('cm-md-dbview-cell--saving')
  try {
    await mutate.update(recordId, { [field]: parsed })
    mutate.refresh()
  } catch (err) {
    wrap.classList.remove('cm-md-dbview-cell--saving')
    wrap.classList.add('cm-md-dbview-cell--error')
    console.warn('[nexus.editor] cell update failed:', err)
  }
}

/** Pick a human title for a record. Strategy: explicit `titleField`
 *  if present and non-empty → first text-shaped field on the
 *  record → the id. Exported so tests can pin the precedence. */
export function recordTitle(
  record: AppliedRecord,
  schema: Record<string, unknown>,
  titleField: string | null,
): string {
  if (titleField) {
    const raw = record[titleField]
    if (raw != null) {
      const s = formatCell(raw, lookupFieldDef(schema, titleField))
      if (s !== '') return s
    }
  }
  for (const k of Object.keys(record)) {
    if (k === 'id' || k === 'deletedAt') continue
    const raw = record[k]
    if (typeof raw !== 'string' || raw.length === 0) continue
    return raw
  }
  return record.id
}

function emptyState(message: string): HTMLElement {
  const el = document.createElement('div')
  el.className = 'cm-md-dbview-empty'
  el.textContent = message
  return el
}

function parseIsoDate(s: string): Date | null {
  // The database engine emits group keys as `YYYY-MM-DD`. Anything
  // else (typically the `(none)` sentinel) routes to the undated
  // bucket.
  if (!/^\d{4}-\d{2}-\d{2}$/.test(s)) return null
  const d = new Date(`${s}T00:00:00Z`)
  return Number.isFinite(d.getTime()) ? d : null
}

function toIsoYmd(d: Date): string {
  const y = d.getUTCFullYear()
  const m = String(d.getUTCMonth() + 1).padStart(2, '0')
  const day = String(d.getUTCDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

function monthName(month: number): string {
  return [
    'January',
    'February',
    'March',
    'April',
    'May',
    'June',
    'July',
    'August',
    'September',
    'October',
    'November',
    'December',
  ][month] ?? ''
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
