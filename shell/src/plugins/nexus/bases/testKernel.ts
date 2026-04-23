// WI-10 closing — `MockKernel` test helper for the bases plugin.
//
// Mirrors the editor plugin's `kernelClient.test.ts` mock-API pattern
// but extracts it as a reusable helper so we don't re-build a kernel
// fake in every test file. The audit's cross-WI observation
// recommended this shape ("a shared vitest harness pattern with a
// `MockKernel` recording IPC ops would close both gaps with high
// reuse — one infra investment, two WIs validated"); WI-11 (canvas)
// and any future shell-side IPC-coverage tests can adopt the same
// helper if they get extracted to a top-level `test-helpers/`
// directory in a follow-up.
//
// Two surfaces:
//
//   1. `makeMockKernel(handlers)` — returns a `KernelAPI` plus a
//      `calls` log of every invocation. Handlers are looked up by
//      `<pluginId>:<commandId>`; missing handlers throw with a clear
//      message so an unexpected call surfaces as an assertion
//      failure rather than a silent `undefined`.
//
//   2. `inMemoryBaseHandlers(initial)` — a default handler set that
//      services the seventeen `base_*` IPC commands against an
//      in-memory base map. Tests use this to round-trip records,
//      properties, and views without a real kernel.

import type { KernelAPI } from '../../../types/plugin'
import type {
  Base,
  BaseRecord,
  BaseSchema,
  BaseView,
} from './kernelClient'

// ─── Recording mock kernel ────────────────────────────────────────────────────

export interface InvokeCall {
  pluginId: string
  commandId: string
  args: unknown
  timeoutMs: number | undefined
}

export interface MockKernel {
  api: KernelAPI
  calls: InvokeCall[]
  /** Convenience filter — `calls.filter(c => c.commandId === id)`. */
  callsTo(commandId: string): InvokeCall[]
  /** Reset the recorded call log. The handler map is unchanged. */
  reset(): void
}

export type MockHandler = (args: unknown) => unknown | Promise<unknown>

/** Build a `KernelAPI` that records every `invoke` and dispatches to
 *  the supplied handler map. `events.on` is a no-op (returns an
 *  unsub) and `available()` resolves true. */
export function makeMockKernel(
  handlers: Record<string, MockHandler> = {},
): MockKernel {
  const calls: InvokeCall[] = []
  const api: KernelAPI = {
    async invoke<T = unknown>(
      pluginId: string,
      commandId: string,
      args?: unknown,
      timeoutMs?: number,
    ): Promise<T> {
      calls.push({ pluginId, commandId, args, timeoutMs })
      const key = `${pluginId}:${commandId}`
      const handler = handlers[key]
      if (!handler) {
        throw new Error(
          `MockKernel: no handler for ${key} (args=${JSON.stringify(args)})`,
        )
      }
      const out = await handler(args)
      return out as T
    },
    async on<T = unknown>(
      _topicPrefix: string,
      _handler: (topic: string, payload: T) => void,
    ): Promise<() => void> {
      return () => {}
    },
    async available(): Promise<boolean> {
      return true
    },
  }
  return {
    api,
    calls,
    callsTo(commandId) {
      return calls.filter((c) => c.commandId === commandId)
    },
    reset() {
      calls.length = 0
    },
  }
}

// ─── In-memory bases handler set ──────────────────────────────────────────────
//
// Drop-in `handlers` map for `makeMockKernel` that stands in for the
// `com.nexus.storage` base_* surface. Tests pass a seed `Record<path,
// Base>` and then exercise the same shell client that production uses.

export interface InMemoryStore {
  bases: Record<string, Base>
}

interface ArgsBase {
  path?: unknown
}

function pathOf(args: unknown): string {
  if (typeof args === 'object' && args && 'path' in args) {
    const p = (args as ArgsBase).path
    if (typeof p === 'string') return p
  }
  throw new Error(`MockKernel: missing string \`path\` arg (got ${JSON.stringify(args)})`)
}

function recordIdOf(args: unknown): string {
  if (typeof args === 'object' && args && 'record_id' in args) {
    const v = (args as { record_id?: unknown }).record_id
    if (typeof v === 'string') return v
  }
  throw new Error(`MockKernel: missing string \`record_id\` arg`)
}

function emptyBase(name: string, schema?: BaseSchema): Base {
  return {
    name,
    schema: schema ?? { fields: {} },
    records: [],
    views: [],
    relations: [],
    metadata: {
      version: '1',
      created_at: 0,
      modified_at: 0,
    },
  }
}

let mintCounter = 0
function mintId(): string {
  mintCounter += 1
  return `mock-${mintCounter.toString().padStart(8, '0')}`
}

/** Build a handler map covering the seventeen `base_*` commands.
 *  Mutates the passed `store` so the same instance can be inspected
 *  by the test after each call. */
export function inMemoryBaseHandlers(
  store: InMemoryStore,
): Record<string, MockHandler> {
  const PLUGIN = 'com.nexus.storage'
  const get = (path: string): Base => {
    const b = store.bases[path]
    if (!b) throw new Error(`MockKernel: no base at ${path}`)
    return b
  }
  return {
    [`${PLUGIN}:base_load`]: (args) => get(pathOf(args)),
    [`${PLUGIN}:base_create`]: (args) => {
      const path = pathOf(args)
      if (store.bases[path]) {
        throw new Error(`MockKernel: base already exists at ${path}`)
      }
      const a = args as { schema?: BaseSchema; seed_records?: BaseRecord[] }
      const base = emptyBase(path, a.schema)
      base.records = (a.seed_records ?? []).map((r) => ({ ...r }))
      store.bases[path] = base
      return base
    },
    [`${PLUGIN}:base_record_create`]: (args) => {
      const base = get(pathOf(args))
      const a = args as { record: BaseRecord }
      const stored: BaseRecord = {
        ...a.record,
        id: a.record.id && a.record.id !== '' ? a.record.id : mintId(),
      }
      base.records.push(stored)
      return stored
    },
    [`${PLUGIN}:base_record_update`]: (args) => {
      const base = get(pathOf(args))
      const id = recordIdOf(args)
      const a = args as { fields: Record<string, unknown> }
      const idx = base.records.findIndex((r) => r.id === id)
      if (idx < 0) throw new Error(`MockKernel: no record ${id}`)
      const merged: BaseRecord = { ...base.records[idx], ...a.fields, id }
      base.records[idx] = merged
      return merged
    },
    [`${PLUGIN}:base_record_delete`]: (args) => {
      const base = get(pathOf(args))
      const id = recordIdOf(args)
      base.records = base.records.filter((r) => r.id !== id)
      return null
    },
    [`${PLUGIN}:base_record_soft_delete`]: (args) => {
      const base = get(pathOf(args))
      const id = recordIdOf(args)
      const idx = base.records.findIndex((r) => r.id === id)
      if (idx < 0) throw new Error(`MockKernel: no record ${id}`)
      base.records[idx] = {
        ...base.records[idx],
        deletedAt: Math.floor(Date.now() / 1000),
      }
      return null
    },
    [`${PLUGIN}:base_record_restore`]: (args) => {
      const base = get(pathOf(args))
      const id = recordIdOf(args)
      const idx = base.records.findIndex((r) => r.id === id)
      if (idx < 0) throw new Error(`MockKernel: no record ${id}`)
      base.records[idx] = { ...base.records[idx], deletedAt: null }
      return null
    },
    [`${PLUGIN}:base_property_create`]: (args) => {
      const base = get(pathOf(args))
      const a = args as { name: string; definition: unknown }
      base.schema = {
        ...base.schema,
        fields: { ...base.schema.fields, [a.name]: a.definition },
      }
      return null
    },
    [`${PLUGIN}:base_property_update`]: (args) => {
      const base = get(pathOf(args))
      const a = args as { name: string; definition: unknown }
      base.schema = {
        ...base.schema,
        fields: { ...base.schema.fields, [a.name]: a.definition },
      }
      return null
    },
    [`${PLUGIN}:base_property_rename`]: (args) => {
      const base = get(pathOf(args))
      const a = args as { old_name: string; new_name: string }
      const fields = { ...base.schema.fields }
      if (!(a.old_name in fields)) {
        throw new Error(`MockKernel: no property ${a.old_name}`)
      }
      fields[a.new_name] = fields[a.old_name]
      delete fields[a.old_name]
      base.schema = { ...base.schema, fields }
      base.records = base.records.map((r) => {
        if (a.old_name in r) {
          const v = r[a.old_name]
          const next: BaseRecord = { ...r, [a.new_name]: v }
          delete next[a.old_name]
          return next
        }
        return r
      })
      return null
    },
    [`${PLUGIN}:base_property_delete`]: (args) => {
      const base = get(pathOf(args))
      const a = args as { name: string }
      const fields = { ...base.schema.fields }
      delete fields[a.name]
      base.schema = { ...base.schema, fields }
      return null
    },
    [`${PLUGIN}:base_view_create`]: (args) => {
      const base = get(pathOf(args))
      const a = args as { view: BaseView }
      if (base.views.some((v) => v.name === a.view.name)) {
        throw new Error(`MockKernel: view ${a.view.name} already exists`)
      }
      base.views.push({ ...a.view })
      return null
    },
    [`${PLUGIN}:base_view_update`]: (args) => {
      const base = get(pathOf(args))
      const a = args as { view: BaseView }
      const idx = base.views.findIndex((v) => v.name === a.view.name)
      if (idx < 0) throw new Error(`MockKernel: no view ${a.view.name}`)
      base.views[idx] = { ...a.view }
      return null
    },
    [`${PLUGIN}:base_view_delete`]: (args) => {
      const base = get(pathOf(args))
      const a = args as { name: string }
      base.views = base.views.filter((v) => v.name !== a.name)
      return null
    },
  }
}
