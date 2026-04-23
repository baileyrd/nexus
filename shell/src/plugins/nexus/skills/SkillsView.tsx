import { useSkillsStore, type SkillEntry, type SkillParameter, type SkillsKernelAPI } from './skillsStore'
import { Icon } from '../../../icons'

interface SkillsViewProps {
  onRefresh: () => void
  kernel: SkillsKernelAPI
}

/**
 * Sidebar listing of `.skill.md` files in the current forge. Rows
 * collapse by default and expand inline on click — the kernel
 * returns the full body in `list`, so the expand panel doesn't need
 * a second IPC call.
 *
 * Per WI-08, an expanded row exposes a "Render" toggle that opens
 * a per-parameter form and submits to `com.nexus.skills::render`.
 * Render output is shown inline below the form.
 */
export function SkillsView({ onRefresh, kernel }: SkillsViewProps) {
  const loading = useSkillsStore((s) => s.loading)
  const loadError = useSkillsStore((s) => s.loadError)
  const skills = useSkillsStore((s) => s.skills)
  const expandedId = useSkillsStore((s) => s.expandedId)
  const toggleExpanded = useSkillsStore((s) => s.toggleExpanded)

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        background: 'var(--bg)',
        color: 'var(--fg)',
        fontFamily: 'var(--f-ui)',
        fontSize: 'var(--ui-size, 13px)',
      }}
    >
      <Header onRefresh={onRefresh} loading={loading} count={skills.length} />
      <div style={{ flex: '1 1 auto', overflow: 'auto' }}>
        {loadError ? (
          <Centered colour="var(--risk)">{loadError}</Centered>
        ) : loading && skills.length === 0 ? (
          <Centered colour="var(--fg-dim)">Loading…</Centered>
        ) : skills.length === 0 ? (
          <Centered colour="var(--fg-dim)">
            No skills. Add a <code>.skill.md</code> under <code>.forge/skills/</code>.
          </Centered>
        ) : (
          skills.map((s) => (
            <SkillRow
              key={s.id}
              skill={s}
              expanded={s.id === expandedId}
              onToggle={() => toggleExpanded(s.id)}
              kernel={kernel}
            />
          ))
        )}
      </div>
    </div>
  )
}

interface HeaderProps {
  onRefresh: () => void
  loading: boolean
  count: number
}

function Header({ onRefresh, loading, count }: HeaderProps) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '6px 10px',
        borderBottom: '1px solid var(--line-soft)',
        background: 'var(--bg-raised)',
        flex: '0 0 auto',
      }}
    >
      <span
        style={{
          flex: '1 1 auto',
          color: 'var(--fg-muted)',
          fontSize: 11,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
        }}
      >
        Skills {count > 0 ? `(${count})` : ''}
      </span>
      <button
        type="button"
        aria-label="Refresh skills"
        title="Reload .forge/skills/"
        onClick={onRefresh}
        disabled={loading}
        onMouseEnter={(e) => {
          if (!loading) (e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-hover)'
        }}
        onMouseLeave={(e) => {
          (e.currentTarget as HTMLButtonElement).style.background = 'transparent'
        }}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          width: 24,
          height: 24,
          padding: 0,
          border: 0,
          background: 'transparent',
          color: 'var(--fg-muted)',
          cursor: loading ? 'default' : 'pointer',
          borderRadius: 'var(--r)',
          opacity: loading ? 0.5 : 1,
        }}
      >
        <Icon name="refresh" size={14} />
      </button>
    </div>
  )
}

interface SkillRowProps {
  skill: SkillEntry
  expanded: boolean
  onToggle: () => void
  kernel: SkillsKernelAPI
}

/** Truncate a multi-line body for the inline preview without breaking
 *  fenced-code blocks visually mid-line. Keeps the first ~40 lines. */
function truncateBody(body: string, maxLines: number): string {
  const lines = body.split(/\r?\n/)
  if (lines.length <= maxLines) return body.trim()
  return lines.slice(0, maxLines).join('\n').trim() + '\n…'
}

function SkillRow({ skill, expanded, onToggle, kernel }: SkillRowProps) {
  return (
    <div
      style={{
        borderBottom: '1px solid var(--line-soft)',
      }}
    >
      <div
        onClick={onToggle}
        role="button"
        aria-expanded={expanded}
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: 3,
          padding: '8px 10px',
          cursor: 'pointer',
          background: expanded ? 'var(--bg-raised)' : 'transparent',
        }}
        onMouseEnter={(e) => {
          if (!expanded) (e.currentTarget as HTMLDivElement).style.background = 'var(--bg-hover)'
        }}
        onMouseLeave={(e) => {
          if (!expanded) (e.currentTarget as HTMLDivElement).style.background = 'transparent'
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span
            aria-hidden
            style={{
              display: 'inline-flex',
              transition: 'transform 80ms',
              transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)',
              color: 'var(--fg-dim)',
            }}
          >
            <Icon name="chev" size={12} />
          </span>
          <span
            style={{
              flex: '1 1 auto',
              fontWeight: 500,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
            title={skill.id}
          >
            {skill.name}
          </span>
          {skill.version ? (
            <span
              style={{
                fontFamily: 'var(--font-mono, monospace)',
                fontSize: 10,
                color: 'var(--fg-dim)',
              }}
            >
              v{skill.version}
            </span>
          ) : null}
        </div>
        {skill.description ? (
          <div
            style={{
              color: 'var(--fg-dim)',
              fontSize: 12,
              lineHeight: 1.35,
              paddingLeft: 18,
            }}
          >
            {skill.description}
          </div>
        ) : null}
      </div>
      {expanded ? <ExpandedPanel skill={skill} kernel={kernel} /> : null}
    </div>
  )
}

function ExpandedPanel({ skill, kernel }: { skill: SkillEntry; kernel: SkillsKernelAPI }) {
  const body = truncateBody(skill.body, 40)
  const renderingId = useSkillsStore((s) => s.renderingId)
  const toggleRenderForm = useSkillsStore((s) => s.toggleRenderForm)
  const renderResult = useSkillsStore((s) => s.renderResults[skill.id])
  const renderError = useSkillsStore((s) => s.renderErrors[skill.id])
  const isFormOpen = renderingId === skill.id

  return (
    <div
      style={{
        padding: '8px 10px 10px 28px',
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
        background: 'var(--bg-raised)',
      }}
    >
      <ChipRow label="tags" items={skill.tags} />
      <ChipRow label="contexts" items={skill.applicableContexts} />
      <ChipRow label="triggers" items={skill.triggers} muted />
      {skill.author ? (
        <div style={{ fontSize: 11, color: 'var(--fg-dim)' }}>by {skill.author}</div>
      ) : null}
      {body ? (
        <pre
          style={{
            margin: 0,
            padding: 8,
            background: 'var(--bg)',
            border: '1px solid var(--line-soft)',
            borderRadius: 'var(--r)',
            fontFamily: 'var(--f-mono, monospace)',
            fontSize: 11,
            lineHeight: 1.45,
            color: 'var(--fg-muted)',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            maxHeight: 240,
            overflow: 'auto',
          }}
        >
          {body}
        </pre>
      ) : null}

      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <button
          type="button"
          onClick={() => toggleRenderForm(skill.id)}
          style={chipButton(isFormOpen)}
        >
          {isFormOpen ? 'Hide render form' : 'Render…'}
        </button>
        {!isFormOpen && skill.parameters.length > 0 ? (
          <span style={{ fontSize: 11, color: 'var(--fg-dim)' }}>
            {skill.parameters.length} parameter{skill.parameters.length === 1 ? '' : 's'}
          </span>
        ) : null}
      </div>

      {isFormOpen ? <RenderForm skill={skill} kernel={kernel} /> : null}

      {renderError ? (
        <div
          role="alert"
          style={{
            padding: 8,
            border: '1px solid var(--risk)',
            borderRadius: 'var(--r)',
            color: 'var(--risk)',
            fontSize: 11,
            whiteSpace: 'pre-wrap',
          }}
        >
          {renderError}
        </div>
      ) : null}

      {renderResult ? <RenderResultPanel skillId={skill.id} body={renderResult.body} /> : null}
    </div>
  )
}

function chipButton(active: boolean): React.CSSProperties {
  return {
    padding: '3px 8px',
    fontSize: 11,
    fontFamily: 'var(--f-ui)',
    background: active ? 'var(--bg-hover)' : 'var(--bg)',
    color: 'var(--fg)',
    border: '1px solid var(--line-soft)',
    borderRadius: 'var(--r)',
    cursor: 'pointer',
  }
}

interface RenderFormProps {
  skill: SkillEntry
  kernel: SkillsKernelAPI
}

/**
 * Per-parameter form. Each `SkillParameter` becomes one input,
 * dispatched on `type`:
 *
 * - `boolean` → checkbox
 * - `enum`    → `<select>` of declared `values`
 * - `number`  → numeric input (parsed to `number` on change)
 * - `list`    → comma-separated text input parsed to `string[]`
 * - default / `string` / unknown → text input
 *
 * The kernel resolves missing values from each parameter's
 * `default` declaration, so the form can be submitted even if the
 * user clears a field.
 */
function RenderForm({ skill, kernel }: RenderFormProps) {
  const draft = useSkillsStore((s) => s.paramDrafts[skill.id]) ?? {}
  const setParamValue = useSkillsStore((s) => s.setParamValue)
  const renderSkill = useSkillsStore((s) => s.renderSkill)
  const rendering = useSkillsStore((s) => s.rendering)
  const isRendering = rendering === skill.id

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    void renderSkill(kernel, skill.id)
  }

  return (
    <form
      onSubmit={handleSubmit}
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
        padding: 8,
        background: 'var(--bg)',
        border: '1px solid var(--line-soft)',
        borderRadius: 'var(--r)',
      }}
    >
      {skill.parameters.length === 0 ? (
        <div style={{ fontSize: 11, color: 'var(--fg-dim)' }}>
          No declared parameters — submit to render the skill body as-is.
        </div>
      ) : (
        skill.parameters.map((p) => (
          <ParamField
            key={p.name}
            param={p}
            value={draft[p.name]}
            onChange={(v) => setParamValue(skill.id, p.name, v)}
          />
        ))
      )}
      <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
        <button
          type="submit"
          disabled={isRendering}
          style={{
            padding: '4px 10px',
            fontSize: 11,
            fontFamily: 'var(--f-ui)',
            background: isRendering ? 'var(--bg-hover)' : 'var(--accent, var(--bg-hover))',
            color: 'var(--fg)',
            border: '1px solid var(--line-soft)',
            borderRadius: 'var(--r)',
            cursor: isRendering ? 'default' : 'pointer',
            opacity: isRendering ? 0.6 : 1,
          }}
        >
          {isRendering ? 'Rendering…' : 'Render'}
        </button>
      </div>
    </form>
  )
}

interface ParamFieldProps {
  param: SkillParameter
  value: unknown
  onChange: (v: unknown) => void
}

function ParamField({ param, value, onChange }: ParamFieldProps) {
  const labelStyle: React.CSSProperties = {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
    fontSize: 11,
  }
  const nameStyle: React.CSSProperties = {
    color: 'var(--fg)',
    fontFamily: 'var(--f-mono, monospace)',
  }
  const helpStyle: React.CSSProperties = {
    color: 'var(--fg-dim)',
    fontSize: 10,
    lineHeight: 1.35,
  }
  const inputStyle: React.CSSProperties = {
    padding: '3px 6px',
    fontSize: 12,
    fontFamily: 'var(--f-mono, monospace)',
    background: 'var(--bg-raised)',
    color: 'var(--fg)',
    border: '1px solid var(--line-soft)',
    borderRadius: 'var(--r)',
  }
  const typeBadge = (
    <span
      style={{
        padding: '0 4px',
        fontSize: 9,
        fontFamily: 'var(--f-mono, monospace)',
        color: 'var(--fg-dim)',
        border: '1px solid var(--line-soft)',
        borderRadius: 999,
        marginLeft: 4,
      }}
    >
      {param.type}
    </span>
  )

  if (param.type === 'boolean') {
    return (
      <label style={{ ...labelStyle, flexDirection: 'row', alignItems: 'center', gap: 6 }}>
        <input
          type="checkbox"
          checked={typeof value === 'boolean' ? value : false}
          onChange={(e) => onChange(e.currentTarget.checked)}
        />
        <span style={nameStyle}>{param.name}</span>
        {typeBadge}
        {param.description ? <span style={helpStyle}>— {param.description}</span> : null}
      </label>
    )
  }

  if (param.type === 'enum' && param.values.length > 0) {
    const v = typeof value === 'string' ? value : value == null ? '' : String(value)
    return (
      <label style={labelStyle}>
        <span>
          <span style={nameStyle}>{param.name}</span>
          {typeBadge}
        </span>
        {param.description ? <span style={helpStyle}>{param.description}</span> : null}
        <select
          value={v}
          onChange={(e) => onChange(e.currentTarget.value)}
          style={inputStyle}
        >
          {param.values.map((opt) => (
            <option key={opt} value={opt}>
              {opt}
            </option>
          ))}
        </select>
      </label>
    )
  }

  if (param.type === 'number') {
    const v = typeof value === 'number' ? String(value) : value == null ? '' : String(value)
    return (
      <label style={labelStyle}>
        <span>
          <span style={nameStyle}>{param.name}</span>
          {typeBadge}
        </span>
        {param.description ? <span style={helpStyle}>{param.description}</span> : null}
        <input
          type="number"
          value={v}
          onChange={(e) => {
            const raw = e.currentTarget.value
            if (raw === '') {
              onChange(undefined)
              return
            }
            const n = Number(raw)
            onChange(Number.isFinite(n) ? n : raw)
          }}
          style={inputStyle}
        />
      </label>
    )
  }

  if (param.type === 'list') {
    // Comma-separated; trimmed; empties dropped. Most skill `list`
    // params are short string lists, so this is the cheapest form.
    const v = Array.isArray(value)
      ? (value as unknown[]).map((x) => String(x)).join(', ')
      : typeof value === 'string'
        ? value
        : ''
    return (
      <label style={labelStyle}>
        <span>
          <span style={nameStyle}>{param.name}</span>
          {typeBadge}
        </span>
        <span style={helpStyle}>
          {param.description
            ? `${param.description} (comma-separated)`
            : 'Comma-separated values'}
        </span>
        <input
          type="text"
          value={v}
          onChange={(e) => {
            const parts = e.currentTarget.value
              .split(',')
              .map((s) => s.trim())
              .filter((s) => s.length > 0)
            onChange(parts)
          }}
          style={inputStyle}
        />
      </label>
    )
  }

  // Fallback: string / unknown.
  const v = typeof value === 'string' ? value : value == null ? '' : String(value)
  return (
    <label style={labelStyle}>
      <span>
        <span style={nameStyle}>{param.name}</span>
        {typeBadge}
      </span>
      {param.description ? <span style={helpStyle}>{param.description}</span> : null}
      <input
        type="text"
        value={v}
        onChange={(e) => onChange(e.currentTarget.value)}
        style={inputStyle}
      />
    </label>
  )
}

function RenderResultPanel({ skillId, body }: { skillId: string; body: string }) {
  const clearRenderResult = useSkillsStore((s) => s.clearRenderResult)
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          fontSize: 10,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
          color: 'var(--fg-dim)',
        }}
      >
        <span style={{ flex: '1 1 auto' }}>Rendered</span>
        <button
          type="button"
          onClick={() => clearRenderResult(skillId)}
          aria-label="Clear render result"
          style={{
            background: 'transparent',
            border: 0,
            color: 'var(--fg-dim)',
            cursor: 'pointer',
            fontSize: 10,
            padding: 0,
          }}
        >
          clear
        </button>
      </div>
      <pre
        style={{
          margin: 0,
          padding: 8,
          background: 'var(--bg)',
          border: '1px solid var(--line-soft)',
          borderRadius: 'var(--r)',
          fontFamily: 'var(--f-mono, monospace)',
          fontSize: 11,
          lineHeight: 1.45,
          color: 'var(--fg)',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
          maxHeight: 320,
          overflow: 'auto',
        }}
      >
        {body}
      </pre>
    </div>
  )
}

interface ChipRowProps {
  label: string
  items: string[]
  muted?: boolean
}

function ChipRow({ label, items, muted }: ChipRowProps) {
  if (items.length === 0) return null
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
      <span
        style={{
          fontSize: 10,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
          color: 'var(--fg-dim)',
          flex: '0 0 auto',
        }}
      >
        {label}
      </span>
      {items.map((item) => (
        <span
          key={item}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            padding: '1px 6px',
            borderRadius: 999,
            fontSize: 10,
            fontFamily: 'var(--font-mono, monospace)',
            background: 'var(--bg)',
            color: muted ? 'var(--fg-dim)' : 'var(--fg-muted)',
            border: '1px solid var(--line-soft)',
          }}
        >
          {item}
        </span>
      ))}
    </div>
  )
}

interface CenteredProps {
  colour: string
  children: React.ReactNode
}

function Centered({ colour, children }: CenteredProps) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        height: '100%',
        padding: 16,
        textAlign: 'center',
        color: colour,
        fontSize: 12,
        lineHeight: 1.4,
      }}
    >
      {children}
    </div>
  )
}
