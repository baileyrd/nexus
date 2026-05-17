// BL-113 follow-up — Launch picker + launch-config form for the
// debugger panel.
//
// When no DAP session is active, the panel renders this component in
// place of the toolbar/status row. Flow:
//
//   1. Fetch `list_adapters` → populate the adapter dropdown, using
//      `metadata.display_name` when present and falling back to the
//      bare `name` otherwise.
//   2. When an adapter is selected, read its
//      `metadata.launch_config_schema` — relative path against the
//      contributing plugin's directory (looked up via
//      `scan_plugin_directory`). If absent / unreadable, fall back to
//      a generic form (program + args + cwd + stop_on_entry).
//   3. Render typed inputs for top-level `type: object` schemas with
//      property kinds `string` / `boolean` / `array<string>`. Defaults
//      from the schema seed the initial form values.
//   4. Submit calls `startSession(api, { adapter, ...formValues })`.
//
// Scope: the schema renderer covers the property kinds debugpy's launch
// spec needs (string / boolean / array<string>). Nested objects /
// `oneOf` / `$ref` chains stay raw-JSON for now.
//
// Schema-file resolution: we use `scan_plugin_directory` (already
// invoked by `communityPluginLoader.ts` at boot) to discover the
// `dir` for `metadata.plugin_id`, then `readTextFile(join(dir,
// schemaPath))`. Failure logs at warn + falls back to generic form.

import { useCallback, useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { readTextFile } from '@tauri-apps/plugin-fs'

import { clientLogger } from '../../../clientLogger'
import {
  listAdapters,
  type DapAdapterEntry,
  type DapKernelAPI,
  type LaunchOpts,
} from './debuggerIpc'
import { useDebuggerStore } from './debuggerStore'

interface LaunchConfigProps {
  api: DapKernelAPI
  onCancel?: () => void
}

/** Minimal JSON-schema shape we render. Anything richer falls back to
 *  the generic form. */
interface FormSchema {
  type?: string
  properties?: Record<string, FormProperty>
  required?: string[]
}

interface FormProperty {
  type?: 'string' | 'boolean' | 'array' | 'integer' | 'number' | string
  default?: unknown
  description?: string
  enum?: string[]
  items?: { type?: string }
}

/** Subset of `CommunityPluginManifest` we need to resolve a plugin
 *  directory. Mirrors `shell/src/host/communityPluginLoader.ts`'s
 *  declared interface. */
interface PluginDirEntry {
  manifest?: { id?: string }
  id?: string
  dir: string
}

/** Path-join shim: schema_path may use either `/` or `\` separators;
 *  we just append using `/` since Tauri's fs accepts both on every
 *  supported platform. */
function joinPath(dir: string, rel: string): string {
  const trimmedDir = dir.replace(/[/\\]+$/, '')
  const trimmedRel = rel.replace(/^[./\\]+/, '')
  return `${trimmedDir}/${trimmedRel}`
}

async function resolvePluginDir(pluginId: string): Promise<string | null> {
  try {
    const entries = await invoke<PluginDirEntry[]>('scan_plugin_directory')
    for (const e of entries ?? []) {
      const id = e.manifest?.id ?? e.id
      if (id === pluginId) return e.dir
    }
  } catch (err) {
    clientLogger.warn('[debugger.launch] scan_plugin_directory failed:', err)
  }
  return null
}

async function loadSchemaFor(adapter: DapAdapterEntry): Promise<FormSchema | null> {
  const md = adapter.metadata
  const schemaRel = md?.launch_config_schema
  const pluginId = md?.plugin_id
  if (typeof schemaRel !== 'string' || typeof pluginId !== 'string') {
    return null
  }
  const pluginDir = await resolvePluginDir(pluginId)
  if (!pluginDir) {
    clientLogger.warn(
      `[debugger.launch] no plugin directory for ${pluginId}; skipping schema`,
    )
    return null
  }
  const schemaPath = joinPath(pluginDir, schemaRel)
  try {
    const text = await readTextFile(schemaPath)
    const parsed: unknown = JSON.parse(text)
    if (typeof parsed === 'object' && parsed !== null) {
      return parsed as FormSchema
    }
    clientLogger.warn(`[debugger.launch] schema not an object: ${schemaPath}`)
    return null
  } catch (err) {
    clientLogger.warn(`[debugger.launch] read schema ${schemaPath} failed:`, err)
    return null
  }
}

/** Seed form values from schema defaults. Pure helper for testing. */
export function seedDefaults(schema: FormSchema | null): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  if (!schema?.properties) return out
  for (const [key, prop] of Object.entries(schema.properties)) {
    if (prop.default !== undefined) {
      out[key] = prop.default
      continue
    }
    switch (prop.type) {
      case 'boolean':
        out[key] = false
        break
      case 'array':
        out[key] = []
        break
      case 'integer':
      case 'number':
        out[key] = 0
        break
      default:
        out[key] = ''
    }
  }
  return out
}

/** Build a LaunchOpts payload from raw form values, hoisting the
 *  known top-level keys into typed slots and routing everything else
 *  into `extra`. Pure helper for testing. */
export function buildLaunchOpts(
  adapter: string,
  values: Record<string, unknown>,
): LaunchOpts {
  const opts: LaunchOpts = { adapter, program: '' }
  const extra: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(values)) {
    switch (k) {
      case 'program':
        opts.program = typeof v === 'string' ? v : String(v ?? '')
        break
      case 'args':
        if (Array.isArray(v)) opts.args = v.map(String)
        break
      case 'cwd':
        if (typeof v === 'string' && v) opts.cwd = v
        break
      case 'env':
        if (v && typeof v === 'object') opts.env = v as Record<string, string>
        break
      case 'stop_on_entry':
      case 'stopOnEntry':
        opts.stop_on_entry = Boolean(v)
        break
      case 'mode':
        if (typeof v === 'string' && v) opts.mode = v
        break
      default:
        extra[k] = v
    }
  }
  if (Object.keys(extra).length > 0) opts.extra = extra
  return opts
}

interface FieldProps {
  name: string
  prop: FormProperty
  value: unknown
  required: boolean
  onChange: (v: unknown) => void
}

function Field({ name, prop, value, required, onChange }: FieldProps) {
  const label = (
    <span style={{ fontSize: '0.85em', color: 'var(--nexus-color-muted)' }}>
      {name}
      {required ? <span style={{ color: 'var(--nexus-color-danger)' }}> *</span> : null}
      {prop.description ? (
        <em style={{ marginLeft: 8 }}>{prop.description}</em>
      ) : null}
    </span>
  )

  if (prop.type === 'boolean') {
    return (
      <label style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 6 }}>
        <input
          type="checkbox"
          checked={Boolean(value)}
          onChange={(e) => onChange(e.currentTarget.checked)}
        />
        {label}
      </label>
    )
  }

  if (prop.type === 'array') {
    const lines = Array.isArray(value) ? (value as unknown[]).map(String).join('\n') : ''
    return (
      <label style={{ display: 'flex', flexDirection: 'column', gap: 4, marginBottom: 6 }}>
        {label}
        <textarea
          rows={3}
          value={lines}
          placeholder="one per line"
          onChange={(e) => onChange(e.currentTarget.value.split('\n').filter((s) => s !== ''))}
          style={{
            padding: '0.3rem 0.5rem',
            border: '1px solid var(--nexus-color-border)',
            borderRadius: 4,
            background: 'var(--nexus-color-bg-elevated)',
            color: 'var(--nexus-color-fg)',
            fontFamily: 'var(--nexus-font-mono, monospace)',
            fontSize: '0.85em',
          }}
        />
      </label>
    )
  }

  // string / fallback
  if (Array.isArray(prop.enum) && prop.enum.length > 0) {
    return (
      <label style={{ display: 'flex', flexDirection: 'column', gap: 4, marginBottom: 6 }}>
        {label}
        <select
          value={typeof value === 'string' ? value : ''}
          onChange={(e) => onChange(e.currentTarget.value)}
          style={{
            padding: '0.3rem 0.5rem',
            border: '1px solid var(--nexus-color-border)',
            borderRadius: 4,
            background: 'var(--nexus-color-bg-elevated)',
            color: 'var(--nexus-color-fg)',
          }}
        >
          {prop.enum.map((opt) => (
            <option key={opt} value={opt}>
              {opt}
            </option>
          ))}
        </select>
      </label>
    )
  }

  return (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 4, marginBottom: 6 }}>
      {label}
      <input
        type="text"
        value={typeof value === 'string' ? value : String(value ?? '')}
        onChange={(e) => onChange(e.currentTarget.value)}
        style={{
          padding: '0.3rem 0.5rem',
          border: '1px solid var(--nexus-color-border)',
          borderRadius: 4,
          background: 'var(--nexus-color-bg-elevated)',
          color: 'var(--nexus-color-fg)',
        }}
      />
    </label>
  )
}

/** Generic fallback when no `launch_config_schema` is available. */
const GENERIC_SCHEMA: FormSchema = {
  type: 'object',
  required: ['program'],
  properties: {
    program: { type: 'string', description: 'Path to the program to debug' },
    args: { type: 'array', items: { type: 'string' } },
    cwd: { type: 'string', description: 'Working directory' },
    stop_on_entry: { type: 'boolean', default: false },
  },
}

export function LaunchConfig({ api, onCancel }: LaunchConfigProps) {
  const startSession = useDebuggerStore((s) => s.startSession)
  const [adapters, setAdapters] = useState<DapAdapterEntry[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [schema, setSchema] = useState<FormSchema | null>(null)
  const [values, setValues] = useState<Record<string, unknown>>({})
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const list = await listAdapters(api)
        if (cancelled) return
        const available = list.filter((a) => !a.disabled)
        setAdapters(available)
        if (available.length > 0 && selected == null) {
          setSelected(available[0].name)
        }
      } catch (err) {
        if (!cancelled) {
          setError(`list_adapters failed: ${(err as Error).message ?? err}`)
        }
      }
    })()
    return () => {
      cancelled = true
    }
    // listAdapters intentionally fetched once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api])

  useEffect(() => {
    let cancelled = false
    if (selected == null) {
      setSchema(null)
      setValues({})
      return
    }
    const adapter = adapters.find((a) => a.name === selected)
    if (!adapter) {
      setSchema(null)
      setValues({})
      return
    }
    void (async () => {
      const loaded = await loadSchemaFor(adapter)
      if (cancelled) return
      const effective = loaded ?? GENERIC_SCHEMA
      setSchema(effective)
      setValues(seedDefaults(effective))
    })()
    return () => {
      cancelled = true
    }
  }, [selected, adapters])

  const required = useMemo(() => new Set(schema?.required ?? []), [schema])
  const properties = useMemo(
    () => Object.entries(schema?.properties ?? {}),
    [schema],
  )

  const submit = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault()
      if (!selected) return
      // Validate required string fields are non-empty.
      const missing: string[] = []
      for (const k of required) {
        const v = values[k]
        if (v == null || v === '' || (Array.isArray(v) && v.length === 0)) {
          missing.push(k)
        }
      }
      if (missing.length > 0) {
        setError(`Missing required field(s): ${missing.join(', ')}`)
        return
      }
      setError(null)
      setBusy(true)
      try {
        const opts = buildLaunchOpts(selected, values)
        await startSession(api, opts)
      } catch (err) {
        setError(`startSession failed: ${(err as Error).message ?? err}`)
      } finally {
        setBusy(false)
      }
    },
    [api, selected, values, required, startSession],
  )

  if (error != null && adapters.length === 0) {
    return (
      <div className="nx-debugger-launch nx-debugger-error">
        <p>{error}</p>
      </div>
    )
  }

  if (adapters.length === 0) {
    return (
      <div className="nx-debugger-launch">
        <p style={{ color: 'var(--nexus-color-muted)' }}>
          No DAP adapters available. Install a debug adapter plugin or
          configure one in <code>dap.toml</code>.
        </p>
      </div>
    )
  }

  return (
    <form
      className="nx-debugger-launch"
      onSubmit={submit}
      style={{ padding: '0.5rem' }}
    >
      <label style={{ display: 'flex', flexDirection: 'column', gap: 4, marginBottom: 12 }}>
        <span style={{ fontSize: '0.85em', color: 'var(--nexus-color-muted)' }}>
          Debug adapter
        </span>
        <select
          value={selected ?? ''}
          onChange={(e) => setSelected(e.currentTarget.value)}
          style={{
            padding: '0.3rem 0.5rem',
            border: '1px solid var(--nexus-color-border)',
            borderRadius: 4,
            background: 'var(--nexus-color-bg-elevated)',
            color: 'var(--nexus-color-fg)',
          }}
        >
          {adapters.map((a) => {
            const display = a.metadata?.display_name ?? a.name
            return (
              <option key={a.name} value={a.name}>
                {display}
                {a.metadata?.display_name && a.metadata.display_name !== a.name
                  ? ` (${a.name})`
                  : ''}
              </option>
            )
          })}
        </select>
      </label>

      {properties.map(([name, prop]) => (
        <Field
          key={name}
          name={name}
          prop={prop}
          value={values[name]}
          required={required.has(name)}
          onChange={(v) => setValues((prev) => ({ ...prev, [name]: v }))}
        />
      ))}

      {error ? (
        <p style={{ color: 'var(--nexus-color-danger)', fontSize: '0.85em' }}>
          {error}
        </p>
      ) : null}

      <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
        <button type="submit" disabled={busy || selected == null}>
          {busy ? 'Starting…' : 'Start debugging'}
        </button>
        {onCancel ? (
          <button type="button" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
        ) : null}
      </div>
    </form>
  )
}
