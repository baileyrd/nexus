// shell/src/plugins/nexus/skills/SkillEditor.tsx
//
// BL-022 — in-app editor for `.skill.md` files.
//
// Save round-trips through `com.nexus.storage::write_file` +
// `com.nexus.skills::reload` (ADR-0005 — no special "save skill" IPC,
// the storage handler stays the single source of write truth).
//
// The component is split into a frontmatter form + body markdown
// textarea so future surfaces (BL-046's code-aware capture per the
// BACKLOG risk #5 note) can swap the body editor without touching
// the frontmatter form. The whole component takes a kernel handle as
// a prop rather than reaching into a module-level singleton, so the
// same surface can be embedded inside any view that has its own
// PluginAPI lifecycle.

import { useSkillsStore, type SkillDraft, type SkillsKernelAPI } from './skillsStore'

interface SkillEditorProps {
  kernel: SkillsKernelAPI
}

/** Top-level editor — reads the active draft from the store and
 *  routes save / cancel / delete back through it. Returns null when
 *  no draft is active so the parent can render unconditionally. */
export function SkillEditor({ kernel }: SkillEditorProps) {
  const draft = useSkillsStore((s) => s.draft)
  const saving = useSkillsStore((s) => s.saving)
  const saveError = useSkillsStore((s) => s.saveError)
  const cancel = useSkillsStore((s) => s.cancelEditor)
  const save = useSkillsStore((s) => s.saveDraft)
  const patch = useSkillsStore((s) => s.patchDraft)

  if (!draft) return null

  const onSave = async (e: React.FormEvent) => {
    e.preventDefault()
    await save(kernel)
  }

  return (
    <form
      onSubmit={onSave}
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
        padding: 12,
        background: 'var(--bg)',
        border: '1px solid var(--line-soft)',
        borderRadius: 'var(--r)',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          paddingBottom: 6,
          borderBottom: '1px solid var(--line-soft)',
        }}
      >
        <span
          style={{
            flex: '1 1 auto',
            fontSize: 12,
            fontWeight: 600,
            color: 'var(--fg)',
          }}
        >
          {draft.isNew ? 'New skill' : `Editing ${draft.name || draft.id}`}
        </span>
        <span style={{ fontSize: 10, color: 'var(--fg-dim)' }}>
          {draft.relpath || '(no path yet)'}
        </span>
      </div>

      <FrontmatterForm draft={draft} onPatch={patch} />

      <BodyField
        body={draft.body}
        onChange={(b) => patch({ body: b })}
      />

      {saveError ? (
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
          {saveError}
        </div>
      ) : null}

      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'flex-end',
          gap: 8,
        }}
      >
        <button
          type="button"
          onClick={cancel}
          disabled={saving}
          style={chipButton(false, saving)}
        >
          Cancel
        </button>
        <button type="submit" disabled={saving} style={primaryButton(saving)}>
          {saving ? 'Saving…' : draft.isNew ? 'Create' : 'Save'}
        </button>
      </div>
    </form>
  )
}

interface FrontmatterFormProps {
  draft: SkillDraft
  onPatch: (patch: Partial<SkillDraft>) => void
}

/** Frontmatter form — typed inputs for the required fields plus
 *  comma-separated text for the list-shaped fields (`tags`,
 *  `applicable_contexts`, `triggers`, `depends_on`). Comma-separated
 *  is the simplest editor that round-trips through `serializeDraft`
 *  cleanly; a chip editor is a future tweak. */
function FrontmatterForm({ draft, onPatch }: FrontmatterFormProps) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
      <Field
        label="id"
        hint={
          draft.isNew
            ? 'kebab-case slug — also drives the filename'
            : 'kebab-case slug; renaming creates a new file'
        }
        value={draft.id}
        onChange={(v) => onPatch({ id: v })}
        mono
      />
      <Field
        label="name"
        value={draft.name}
        onChange={(v) => onPatch({ name: v })}
      />
      <Field
        label="description"
        value={draft.description}
        onChange={(v) => onPatch({ description: v })}
        wide
      />
      <Field
        label="version"
        value={draft.version}
        onChange={(v) => onPatch({ version: v })}
        mono
      />
      <Field
        label="author"
        value={draft.author}
        onChange={(v) => onPatch({ author: v })}
      />
      <Field
        label="created"
        hint="YYYY-MM-DD"
        value={draft.created}
        onChange={(v) => onPatch({ created: v })}
        mono
      />
      <ListField
        label="tags"
        items={draft.tags}
        onChange={(items) => onPatch({ tags: items })}
      />
      <ListField
        label="applicable_contexts"
        hint="ai-chat / editor / pull-request / terminal / agent"
        items={draft.applicableContexts}
        onChange={(items) => onPatch({ applicableContexts: items })}
      />
      <ListField
        label="triggers"
        hint="phrases that auto-activate the skill"
        items={draft.triggers}
        onChange={(items) => onPatch({ triggers: items })}
      />
      <ListField
        label="depends_on"
        hint="other skill ids to layer in (BL-021)"
        items={draft.dependsOn}
        onChange={(items) => onPatch({ dependsOn: items })}
      />
    </div>
  )
}

function BodyField({
  body,
  onChange,
}: {
  body: string
  onChange: (v: string) => void
}) {
  return (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <span
        style={{
          fontSize: 10,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
          color: 'var(--fg-dim)',
        }}
      >
        body (markdown)
      </span>
      <textarea
        value={body}
        onChange={(e) => onChange(e.currentTarget.value)}
        rows={10}
        spellCheck={false}
        style={{
          padding: 8,
          background: 'var(--bg-raised)',
          color: 'var(--fg)',
          border: '1px solid var(--line-soft)',
          borderRadius: 'var(--r)',
          fontFamily: 'var(--f-mono, monospace)',
          fontSize: 12,
          lineHeight: 1.45,
          resize: 'vertical',
        }}
      />
    </label>
  )
}

interface FieldProps {
  label: string
  value: string
  onChange: (v: string) => void
  hint?: string
  mono?: boolean
  wide?: boolean
}

function Field({ label, value, onChange, hint, mono, wide }: FieldProps) {
  return (
    <label
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 2,
        gridColumn: wide ? '1 / -1' : undefined,
      }}
    >
      <span
        style={{
          fontSize: 10,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
          color: 'var(--fg-dim)',
          fontFamily: 'var(--f-mono, monospace)',
        }}
      >
        {label}
      </span>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.currentTarget.value)}
        spellCheck={false}
        style={{
          padding: '4px 8px',
          background: 'var(--bg-raised)',
          color: 'var(--fg)',
          border: '1px solid var(--line-soft)',
          borderRadius: 'var(--r)',
          fontFamily: mono ? 'var(--f-mono, monospace)' : 'var(--f-ui)',
          fontSize: 12,
        }}
      />
      {hint ? (
        <span style={{ fontSize: 10, color: 'var(--fg-dim)' }}>{hint}</span>
      ) : null}
    </label>
  )
}

interface ListFieldProps {
  label: string
  items: string[]
  onChange: (items: string[]) => void
  hint?: string
}

function ListField({ label, items, onChange, hint }: ListFieldProps) {
  // Comma-separated text edit. Round-trips through trim + filter so
  // trailing commas don't introduce ghost entries.
  const text = items.join(', ')
  return (
    <Field
      label={label}
      hint={hint ? `${hint} (comma-separated)` : 'Comma-separated'}
      value={text}
      onChange={(raw) => {
        const parts = raw
          .split(',')
          .map((s) => s.trim())
          .filter((s) => s.length > 0)
        onChange(parts)
      }}
      mono
    />
  )
}

function chipButton(active: boolean, disabled: boolean): React.CSSProperties {
  return {
    padding: '4px 10px',
    fontSize: 11,
    fontFamily: 'var(--f-ui)',
    background: active ? 'var(--bg-hover)' : 'var(--bg)',
    color: 'var(--fg)',
    border: '1px solid var(--line-soft)',
    borderRadius: 'var(--r)',
    cursor: disabled ? 'default' : 'pointer',
    opacity: disabled ? 0.6 : 1,
  }
}

function primaryButton(disabled: boolean): React.CSSProperties {
  return {
    padding: '4px 12px',
    fontSize: 11,
    fontFamily: 'var(--f-ui)',
    background: disabled ? 'var(--bg-hover)' : 'var(--accent, var(--bg-hover))',
    color: 'var(--fg)',
    border: '1px solid var(--line-soft)',
    borderRadius: 'var(--r)',
    cursor: disabled ? 'default' : 'pointer',
    opacity: disabled ? 0.6 : 1,
    fontWeight: 500,
  }
}
