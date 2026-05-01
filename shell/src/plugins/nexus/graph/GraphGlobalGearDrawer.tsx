import { useGlobalGraphStore, type GlobalGraphSettings } from './graphGlobalStore'

interface Props {
  open: boolean
  onClose: () => void
}

export function GraphGlobalGearDrawer({ open, onClose }: Props) {
  const settings = useGlobalGraphStore((s) => s.settings)
  const patch = useGlobalGraphStore((s) => s.patchSettings)
  const reset = useGlobalGraphStore((s) => s.resetSettings)

  if (!open) return null

  return (
    <div
      style={{
        position: 'absolute',
        top: 36,
        right: 8,
        width: 260,
        maxHeight: 'calc(100% - 56px)',
        overflowY: 'auto',
        background: 'var(--background-secondary)',
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 6,
        padding: 12,
        zIndex: 10,
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
        boxShadow: '0 4px 12px rgba(0,0,0,0.3)',
      }}
    >
      <Header label="Filters" />
      <Field label="Path filter">
        <input
          type="text"
          value={settings.pathFilter}
          placeholder="substring or glob"
          onChange={(e) => patch({ pathFilter: e.target.value })}
          style={inputStyle}
        />
      </Field>
      <Toggle
        label="Include unresolved links"
        value={settings.includeUnresolved}
        onChange={(v) => patch({ includeUnresolved: v })}
      />
      <Toggle
        label="Include orphan nodes"
        value={settings.includeOrphans}
        onChange={(v) => patch({ includeOrphans: v })}
      />

      <Header label="Groups" />
      <Toggle
        label="Colour by folder"
        value={settings.colourByFolder}
        onChange={(v) => patch({ colourByFolder: v })}
      />

      <Header label="Display" />
      <Toggle
        label="Show labels"
        value={settings.showLabels}
        onChange={(v) => patch({ showLabels: v })}
      />
      <Toggle
        label="Freeze simulation"
        value={settings.freeze}
        onChange={(v) => patch({ freeze: v })}
      />

      <Header label="Forces" />
      <Slider
        label="Center gravity"
        value={settings.centerGravity}
        min={0}
        max={0.2}
        step={0.005}
        onChange={(v) => patch({ centerGravity: v })}
      />
      <Slider
        label="Link distance"
        value={settings.linkDistance}
        min={20}
        max={200}
        step={1}
        onChange={(v) => patch({ linkDistance: v })}
      />
      <Slider
        label="Link strength"
        value={settings.linkStrength}
        min={0.01}
        max={1}
        step={0.01}
        onChange={(v) => patch({ linkStrength: v })}
      />
      <Slider
        label="Repulsion"
        value={settings.repulsion}
        min={50}
        max={800}
        step={10}
        onChange={(v) => patch({ repulsion: v })}
      />

      <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
        <button onClick={reset} style={buttonStyle}>
          Reset
        </button>
        <button onClick={onClose} style={buttonStyle}>
          Close
        </button>
      </div>
    </div>
  )
}

function Header({ label }: { label: string }) {
  return (
    <div
      style={{
        marginTop: 8,
        marginBottom: 4,
        color: 'var(--text-muted)',
        textTransform: 'uppercase',
        fontSize: 10,
        letterSpacing: 0.6,
      }}
    >
      {label}
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label style={{ display: 'block', marginBottom: 6 }}>
      <div style={{ color: 'var(--text-faint)', marginBottom: 2 }}>{label}</div>
      {children}
    </label>
  )
}

function Toggle({
  label,
  value,
  onChange,
}: {
  label: string
  value: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <label
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        marginBottom: 4,
        color: 'var(--text-normal)',
        cursor: 'pointer',
      }}
    >
      <input
        type="checkbox"
        checked={value}
        onChange={(e) => onChange(e.target.checked)}
      />
      {label}
    </label>
  )
}

function Slider({
  label,
  value,
  min,
  max,
  step,
  onChange,
}: {
  label: string
  value: number
  min: number
  max: number
  step: number
  onChange: (v: number) => void
}) {
  return (
    <div style={{ marginBottom: 6 }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          color: 'var(--text-faint)',
        }}
      >
        <span>{label}</span>
        <span>{value.toFixed(step < 0.1 ? 3 : step < 1 ? 2 : 0)}</span>
      </div>
      <input
        type="range"
        value={value}
        min={min}
        max={max}
        step={step}
        onChange={(e) => onChange(Number(e.target.value))}
        style={{ width: '100%' }}
      />
    </div>
  )
}

const inputStyle: React.CSSProperties = {
  width: '100%',
  background: 'var(--background-primary)',
  color: 'var(--text-normal)',
  border: '1px solid var(--background-modifier-border)',
  borderRadius: 4,
  padding: '4px 6px',
  fontFamily: 'var(--font-interface)',
  fontSize: 12,
}

const buttonStyle: React.CSSProperties = {
  background: 'var(--background-primary)',
  color: 'var(--text-normal)',
  border: '1px solid var(--background-modifier-border)',
  borderRadius: 4,
  padding: '4px 10px',
  fontFamily: 'var(--font-interface)',
  fontSize: 12,
  cursor: 'pointer',
}

export type { GlobalGraphSettings }
