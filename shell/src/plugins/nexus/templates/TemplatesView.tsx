import { useState } from 'react'
import { useTemplatesStore, type TemplateEntry, type TemplateParameter } from './templatesStore'

export interface TemplatesKernelAPI {
  invoke<T = unknown>(pluginId: string, commandId: string, args?: unknown): Promise<T>
}

export interface TemplatesViewProps {
  kernel: TemplatesKernelAPI
  /** Refetch the listing — wired by the plugin module. */
  onRefresh: () => void
  /** Show a transient toast — wired to api.notifications.show. */
  notify: (message: string, type?: 'success' | 'error' | 'warning' | 'info') => void
  /** Open a forge-relative file in the editor. Wired to nexus.files
   *  command if available. */
  openFile?: (forgeRelativePath: string) => void
}

const PLUGIN_ID = 'com.nexus.templates'
const HANDLER_APPLY = 'apply'

const EMPTY_FORM: Record<string, string> = Object.freeze({}) as Record<string, string>

interface ApplyResult {
  name: string
  path: string
  absolute_path: string
}

/**
 * Side-panel view for templates. Shows every available template; click
 * to expand into a parameter form; "Apply" calls the IPC handler.
 *
 * Intentionally tiny — no fancy editor, no preview pane. The CLI and
 * command palette cover the same surface for power users.
 */
export function TemplatesView({
  kernel,
  onRefresh,
  notify,
  openFile,
}: TemplatesViewProps): JSX.Element {
  const templates = useTemplatesStore((s) => s.templates)
  const loading = useTemplatesStore((s) => s.loading)
  const loadError = useTemplatesStore((s) => s.loadError)
  const selected = useTemplatesStore((s) => s.selected)
  const select = useTemplatesStore((s) => s.select)
  const lastApplied = useTemplatesStore((s) => s.lastApplied)

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        overflow: 'hidden',
      }}
    >
      <header
        style={{
          padding: '8px 12px',
          borderBottom: '1px solid var(--color-border-default, #2a2a2a)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
        }}
      >
        <span style={{ fontWeight: 600 }}>Templates</span>
        <button
          type="button"
          onClick={onRefresh}
          disabled={loading}
          style={{ fontSize: 12 }}
          aria-label="Refresh templates"
        >
          {loading ? 'Loading…' : 'Refresh'}
        </button>
      </header>

      {loadError && (
        <div
          role="alert"
          style={{
            padding: '8px 12px',
            color: 'var(--color-danger-text, #ff6b6b)',
            fontSize: 12,
          }}
        >
          {loadError}
        </div>
      )}

      {lastApplied && (
        <div
          style={{
            padding: '6px 12px',
            background: 'var(--color-success-bg, rgba(80, 200, 120, 0.12))',
            color: 'var(--color-success-text, #50c878)',
            fontSize: 12,
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
          }}
        >
          <span>Last applied: {lastApplied}</span>
          {openFile && (
            <button
              type="button"
              onClick={() => openFile(lastApplied)}
              style={{ fontSize: 11 }}
            >
              Open
            </button>
          )}
        </div>
      )}

      <ul
        style={{
          listStyle: 'none',
          margin: 0,
          padding: 0,
          overflow: 'auto',
          flex: 1,
        }}
      >
        {templates.length === 0 && !loading && !loadError && (
          <li style={{ padding: 12, color: 'var(--color-text-secondary, #888)' }}>
            No templates available.
          </li>
        )}
        {templates.map((tpl) => (
          <TemplateRow
            key={tpl.name}
            tpl={tpl}
            expanded={selected === tpl.name}
            onToggle={() => select(selected === tpl.name ? null : tpl.name)}
            kernel={kernel}
            notify={notify}
            openFile={openFile}
          />
        ))}
      </ul>
    </div>
  )
}

interface TemplateRowProps {
  tpl: TemplateEntry
  expanded: boolean
  onToggle: () => void
  kernel: TemplatesKernelAPI
  notify: TemplatesViewProps['notify']
  openFile: TemplatesViewProps['openFile']
}

function TemplateRow({
  tpl,
  expanded,
  onToggle,
  kernel,
  notify,
  openFile,
}: TemplateRowProps): JSX.Element {
  return (
    <li
      style={{
        borderBottom: '1px solid var(--color-border-muted, #1f1f1f)',
      }}
    >
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={expanded}
        style={{
          width: '100%',
          textAlign: 'left',
          padding: '10px 12px',
          background: 'transparent',
          border: 'none',
          cursor: 'pointer',
          color: 'inherit',
          display: 'flex',
          flexDirection: 'column',
          gap: 2,
        }}
      >
        <span style={{ fontWeight: 600 }}>{tpl.name}</span>
        {tpl.description && (
          <span
            style={{
              fontSize: 11,
              color: 'var(--color-text-secondary, #888)',
            }}
          >
            {tpl.description}
          </span>
        )}
      </button>
      {expanded && (
        <TemplateForm
          tpl={tpl}
          kernel={kernel}
          notify={notify}
          openFile={openFile}
        />
      )}
    </li>
  )
}

interface TemplateFormProps {
  tpl: TemplateEntry
  kernel: TemplatesKernelAPI
  notify: TemplatesViewProps['notify']
  openFile: TemplatesViewProps['openFile']
}

function TemplateForm({
  tpl,
  kernel,
  notify,
  openFile,
}: TemplateFormProps): JSX.Element {
  const formValues = useTemplatesStore((s) => s.formValues[tpl.name]) ?? EMPTY_FORM
  const setFormValue = useTemplatesStore((s) => s.setFormValue)
  const clearForm = useTemplatesStore((s) => s.clearForm)
  const setLastApplied = useTemplatesStore((s) => s.setLastApplied)
  const [busy, setBusy] = useState(false)

  const params = tpl.parameters ?? []

  const handleApply = async (): Promise<void> => {
    // Required-field check before round-tripping through IPC.
    const missing = params
      .filter((p) => p.required && (formValues[p.name] ?? '').trim() === '')
      .map((p) => p.name)
    if (missing.length > 0) {
      notify(`Missing required parameter(s): ${missing.join(', ')}`, 'error')
      return
    }

    // Drop blank optional values so defaults kick in server-side.
    const args: Record<string, string> = {}
    for (const p of params) {
      const v = formValues[p.name] ?? ''
      if (v.trim() !== '') args[p.name] = v
    }

    setBusy(true)
    try {
      const result = await kernel.invoke<ApplyResult>(PLUGIN_ID, HANDLER_APPLY, {
        name: tpl.name,
        args,
      })
      setLastApplied(result.path)
      clearForm(tpl.name)
      notify(`Created ${result.path}`, 'success')
      if (openFile) openFile(result.path)
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      notify(`Apply failed: ${message}`, 'error')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      style={{
        padding: '8px 12px 12px 12px',
        background: 'var(--color-background-elevated, rgba(255,255,255,0.02))',
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
      }}
    >
      {params.length === 0 ? (
        <span style={{ fontSize: 11, color: 'var(--color-text-secondary, #888)' }}>
          No parameters — this template applies as-is.
        </span>
      ) : (
        params.map((p) => (
          <ParameterField
            key={p.name}
            param={p}
            value={formValues[p.name] ?? ''}
            onChange={(v) => setFormValue(tpl.name, p.name, v)}
          />
        ))
      )}
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button
          type="button"
          onClick={() => clearForm(tpl.name)}
          disabled={busy || params.length === 0}
          style={{ fontSize: 12 }}
        >
          Reset
        </button>
        <button
          type="button"
          onClick={() => void handleApply()}
          disabled={busy}
          style={{ fontSize: 12, fontWeight: 600 }}
        >
          {busy ? 'Applying…' : 'Apply'}
        </button>
      </div>
    </div>
  )
}

interface ParameterFieldProps {
  param: TemplateParameter
  value: string
  onChange: (value: string) => void
}

function ParameterField({
  param,
  value,
  onChange,
}: ParameterFieldProps): JSX.Element {
  const placeholder = param.default ?? ''
  return (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <span style={{ fontSize: 11 }}>
        {param.name}
        {param.required && (
          <span style={{ color: 'var(--color-danger-text, #ff6b6b)' }}>*</span>
        )}
        {param.description && (
          <span
            style={{
              marginLeft: 6,
              color: 'var(--color-text-secondary, #888)',
              fontWeight: 'normal',
            }}
          >
            — {param.description}
          </span>
        )}
      </span>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        style={{
          padding: '4px 6px',
          fontSize: 12,
          background: 'var(--color-background-base, #111)',
          color: 'inherit',
          border: '1px solid var(--color-border-default, #333)',
          borderRadius: 3,
        }}
      />
    </label>
  )
}
