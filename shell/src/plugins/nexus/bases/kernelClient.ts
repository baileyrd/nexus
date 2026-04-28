// Thin wrappers over the com.nexus.storage base_* IPC handlers (ids
// 16, 17, 21, 26, 40–48). The shapes below mirror nexus_types::bases
// on the wire — fields we don't yet render are typed-but-unused so
// later phases don't have to widen every consumer.

import type { PluginAPI } from '../../../types/plugin'

export const STORAGE_PLUGIN_ID = 'com.nexus.storage'
export const DATABASE_PLUGIN_ID = 'com.nexus.database'

export type ViewType = 'table' | 'kanban' | 'calendar' | 'gallery' | 'list' | 'timeline'

export interface SortRule {
  field: string
  direction: string // "asc" | "desc" (kernel default "asc")
}

export interface FilterRule {
  field: string
  operator: string
  value: unknown
}

export interface BaseView {
  name: string
  type: ViewType
  fields?: string[]
  sort?: SortRule[]
  filter?: FilterRule[]
  groupField?: string
  dateField?: string
  /** Timeline views only — the record field holding the end-of-bar
   *  date. Paired with `dateField` as the start. */
  endField?: string
}

export interface BaseRecord {
  id: string
  /** Soft-delete timestamp (Unix epoch seconds). `undefined`/`null`
   *  = live record. Views filter records with this set. */
  deletedAt?: number | null
  /** All non-id record fields flatten into the same object on the
   *  wire thanks to `#[serde(flatten)]`. */
  [field: string]: unknown
}

export interface BaseSchema {
  version?: string
  /** Field name → field definition (opaque to the shell at this
   *  phase; Phase 2 introduces a narrower FieldDefinition type). */
  fields: Record<string, unknown>
}

export interface BaseMetadata {
  version: string
  created_at: number
  modified_at: number
}

export interface BaseRelation {
  name: string
  type: string
  sourceField: string
  targetBase: string
  targetField: string
}

export interface Base {
  name: string
  schema: BaseSchema
  records: BaseRecord[]
  views: BaseView[]
  relations: BaseRelation[]
  metadata: BaseMetadata
}

export interface CsvImportResult {
  records: BaseRecord[]
  imported: number
  skipped: number
  errors: Array<[number, string]>
}

/** Wire shape returned by the `obsidian_base_query` IPC handler.
 *  See ADR 0019. Records come back already projected as one object
 *  per matched note; the shell adapts this into a synthetic `Base`
 *  so the existing view layer renders both formats unchanged. */
export interface ObsidianBaseQueryResult {
  columns: string[]
  display_names: Record<string, string>
  rows: Array<{ id: string; fields: Record<string, unknown> }>
  views: Array<{
    name: string
    type: string
    order?: string[]
    sort?: Array<{ property: string; direction?: 'ASC' | 'DESC' }>
    groupBy?: string
    limit?: number
  }>
  unsupported_filters: string[]
}

/** Result of loading a `.base` file: the adapted `Base` plus the
 *  list of expressions the v1 grammar could not evaluate (surfaced
 *  by the UI as a banner). */
export interface ObsidianBaseLoad {
  base: Base
  unsupportedFilters: string[]
}

export interface BasesKernelClient {
  /** Load the full base (schema + records + views + relations) from
   *  a `.bases` directory. */
  loadBase(relpath: string): Promise<Base>
  /** Load and evaluate an Obsidian single-file `.base` (read-only).
   *  Adapts the query result into a `Base` shape with a synthesized
   *  schema so existing views work unchanged. */
  loadObsidianBase(relpath: string): Promise<ObsidianBaseLoad>
  /** Create a new `.bases` directory at `relpath` with the given
   *  schema (and optional seed records). Rejects if `relpath`
   *  already exists. Returns the freshly-created base. */
  createBase(
    relpath: string,
    schema: BaseSchema,
    seedRecords?: BaseRecord[],
  ): Promise<Base>
  /** Append a new record (kernel mints a v4 UUID if `id` is empty).
   *  Returns the stored record. */
  createRecord(relpath: string, record: BaseRecord): Promise<BaseRecord>
  /** Shallow-merge `fields` into the record `record_id`. Returns the
   *  updated record. */
  updateRecord(
    relpath: string,
    recordId: string,
    fields: Record<string, unknown>,
  ): Promise<BaseRecord>
  /** Remove the record; missing ids are a no-op. */
  deleteRecord(relpath: string, recordId: string): Promise<void>
  /** Set `deleted_at` on the record but keep it on disk. Views
   *  filter soft-deleted records from their visible set. */
  softDeleteRecord(relpath: string, recordId: string): Promise<void>
  /** Clear `deleted_at` on a soft-deleted record. */
  restoreRecord(relpath: string, recordId: string): Promise<void>
  createProperty(relpath: string, name: string, definition: unknown): Promise<void>
  /** Replace a property definition. When `migrateValues` is true the
   *  kernel walks every record and coerces stored values to the new
   *  type; values that cannot coerce are dropped to null. */
  updateProperty(
    relpath: string,
    name: string,
    definition: unknown,
    migrateValues?: boolean,
  ): Promise<void>
  /** Rename a schema column. Moves the field definition and updates
   *  every record's fields map in place. Rejects when `newName`
   *  already exists. */
  renameProperty(relpath: string, oldName: string, newName: string): Promise<void>
  deleteProperty(relpath: string, name: string): Promise<void>
  createView(relpath: string, view: BaseView): Promise<void>
  updateView(relpath: string, view: BaseView): Promise<void>
  deleteView(relpath: string, name: string): Promise<void>
  /** Parse CSV bytes into records. `has_header=true` matches the
   *  header row against `fieldNames`; otherwise columns land
   *  positionally. The returned records still need to be persisted
   *  via `createRecord` — the kernel handler is pure parsing. */
  csvImport(
    csvBytes: Uint8Array,
    fieldNames: string[],
    hasHeader: boolean,
  ): Promise<CsvImportResult>
  /** Serialize records to CSV bytes (header + one row per record). */
  csvExport(records: BaseRecord[], fieldNames: string[]): Promise<Uint8Array>
  /** Evaluate a formula expression against a record's fields;
   *  returns the display string the formula engine would render. */
  formulaEval(
    expression: string,
    fields: Record<string, unknown>,
  ): Promise<string>
}

export function makeBasesKernelClient(kernel: PluginAPI['kernel']): BasesKernelClient {
  return {
    async loadBase(relpath) {
      return kernel.invoke<Base>(STORAGE_PLUGIN_ID, 'base_load', { path: relpath })
    },
    async loadObsidianBase(relpath) {
      const resp = await kernel.invoke<ObsidianBaseQueryResult>(
        STORAGE_PLUGIN_ID,
        'obsidian_base_query',
        { path: relpath },
      )
      return adaptObsidianBase(relpath, resp)
    },
    async createBase(relpath, schema, seedRecords = []) {
      return kernel.invoke<Base>(STORAGE_PLUGIN_ID, 'base_create', {
        path: relpath,
        schema,
        seed_records: seedRecords,
      })
    },
    async createRecord(relpath, record) {
      return kernel.invoke<BaseRecord>(STORAGE_PLUGIN_ID, 'base_record_create', {
        path: relpath,
        record,
      })
    },
    async updateRecord(relpath, recordId, fields) {
      return kernel.invoke<BaseRecord>(STORAGE_PLUGIN_ID, 'base_record_update', {
        path: relpath,
        record_id: recordId,
        fields,
      })
    },
    async deleteRecord(relpath, recordId) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_record_delete', {
        path: relpath,
        record_id: recordId,
      })
    },
    async softDeleteRecord(relpath, recordId) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_record_soft_delete', {
        path: relpath,
        record_id: recordId,
      })
    },
    async restoreRecord(relpath, recordId) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_record_restore', {
        path: relpath,
        record_id: recordId,
      })
    },
    async createProperty(relpath, name, definition) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_property_create', {
        path: relpath,
        name,
        definition,
      })
    },
    async updateProperty(relpath, name, definition, migrateValues = false) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_property_update', {
        path: relpath,
        name,
        definition,
        migrate_values: migrateValues,
      })
    },
    async renameProperty(relpath, oldName, newName) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_property_rename', {
        path: relpath,
        old_name: oldName,
        new_name: newName,
      })
    },
    async deleteProperty(relpath, name) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_property_delete', {
        path: relpath,
        name,
      })
    },
    async createView(relpath, view) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_view_create', {
        path: relpath,
        view,
      })
    },
    async updateView(relpath, view) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_view_update', {
        path: relpath,
        view,
      })
    },
    async deleteView(relpath, name) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_view_delete', {
        path: relpath,
        name,
      })
    },
    async csvImport(csvBytes, fieldNames, hasHeader) {
      const resp = await kernel.invoke<{
        records: BaseRecord[]
        imported: number
        skipped: number
        errors: Array<[number, string]>
      }>(DATABASE_PLUGIN_ID, 'csv_import', {
        csv_bytes: Array.from(csvBytes),
        field_names: fieldNames,
        has_header: hasHeader,
      })
      return resp
    },
    async csvExport(records, fieldNames) {
      const resp = await kernel.invoke<{ csv_bytes: number[]; count: number }>(
        DATABASE_PLUGIN_ID,
        'csv_export',
        { records, field_names: fieldNames },
      )
      return Uint8Array.from(resp.csv_bytes)
    },
    async formulaEval(expression, fields) {
      const resp = await kernel.invoke<{ display: string }>(
        DATABASE_PLUGIN_ID,
        'formula_eval',
        { expression, fields },
      )
      return resp.display
    },
  }
}

/** Map an Obsidian `.base` view's `type` string to the shell's
 *  `ViewType`. Cards become a gallery view; board becomes kanban;
 *  unknown types fall back to table so the leaf still renders. */
function mapViewType(t: string): ViewType {
  switch (t) {
    case 'table':
      return 'table'
    case 'cards':
    case 'gallery':
      return 'gallery'
    case 'board':
    case 'kanban':
      return 'kanban'
    case 'list':
      return 'list'
    case 'calendar':
      return 'calendar'
    case 'timeline':
      return 'timeline'
    default:
      return 'table'
  }
}

/** Adapt the IPC response into the `Base` shape the view layer
 *  expects. Synthesizes a schema where every column is a plain
 *  `text` field carrying the optional `displayName` from the
 *  `.base` file. Records are taken verbatim from `rows`. */
function adaptObsidianBase(relpath: string, resp: ObsidianBaseQueryResult): ObsidianBaseLoad {
  const fields: Record<string, unknown> = {}
  for (const col of resp.columns) {
    const def: Record<string, unknown> = { type: 'text' }
    const dn = resp.display_names?.[col]
    if (dn) def.displayName = dn
    fields[col] = def
  }
  const records: BaseRecord[] = resp.rows.map((row) => ({
    id: row.id,
    deletedAt: null,
    ...row.fields,
  }))
  const views: BaseView[] = resp.views.map((v) => ({
    name: v.name,
    type: mapViewType(v.type),
    fields: v.order ?? [],
    sort: (v.sort ?? []).map((s) => ({
      field: s.property,
      direction: (s.direction ?? 'ASC').toLowerCase(),
    })),
    groupField: v.groupBy,
  }))
  const stem = relpath.split('/').pop()?.replace(/\.base$/i, '') ?? 'Base'
  const base: Base = {
    name: stem,
    schema: { version: '1.0', fields },
    records,
    views,
    relations: [],
    metadata: { version: '1.0', created_at: 0, modified_at: 0 },
  }
  return { base, unsupportedFilters: resp.unsupported_filters ?? [] }
}
