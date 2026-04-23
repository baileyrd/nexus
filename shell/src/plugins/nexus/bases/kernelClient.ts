// Thin wrappers over the com.nexus.storage base_* IPC handlers (ids
// 16, 17, 21, 26, 40–48). The shapes below mirror nexus_types::bases
// on the wire — fields we don't yet render are typed-but-unused so
// later phases don't have to widen every consumer.

import type { PluginAPI } from '../../../types/plugin'

export const STORAGE_PLUGIN_ID = 'com.nexus.storage'
export const DATABASE_PLUGIN_ID = 'com.nexus.database'

export type ViewType = 'table' | 'kanban' | 'calendar' | 'gallery'

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
}

export interface BaseRecord {
  id: string
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

export interface BasesKernelClient {
  /** Load the full base (schema + records + views + relations) from
   *  a `.bases` directory. */
  loadBase(relpath: string): Promise<Base>
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
  createProperty(relpath: string, name: string, definition: unknown): Promise<void>
  updateProperty(relpath: string, name: string, definition: unknown): Promise<void>
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
    async createProperty(relpath, name, definition) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_property_create', {
        path: relpath,
        name,
        definition,
      })
    },
    async updateProperty(relpath, name, definition) {
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'base_property_update', {
        path: relpath,
        name,
        definition,
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
