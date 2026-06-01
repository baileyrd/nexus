// R8 / #191 — settings UI cell primitives lifted out of `SettingsPanelView.tsx`
// (which exceeded 3,300 LoC). These are the leaf cells the tabs compose:
// `StubRow` is the title/control/description layout, `StubToggle` is the
// switch primitive, and the `Wired*` family binds a single `configStore`
// key to its corresponding input control. The "Stub" name is a misnomer —
// `StubRow` is the row primitive used by every tab, not just the
// placeholder pages. Naming kept as-is to minimise the diff at consumer
// sites.
//
// Imported by `SettingsPanelView.tsx` and `SettingsStubPages.tsx`.

import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { useConfigStore, useConfigValue } from '../../../stores/configStore'
import type { PluginAPI } from '../../../types/plugin'

export function StubToggle({
  on,
  label,
  onClick,
}: {
  on: boolean
  label: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title="Coming soon"
      aria-label={label}
      style={{
        width: 36,
        height: 20,
        borderRadius: 10,
        border: '1px solid var(--background-modifier-border)',
        background: on ? 'var(--interactive-accent)' : 'var(--background-modifier-hover)',
        cursor: 'pointer',
        position: 'relative',
        padding: 0,
      }}
    >
      <span
        style={{
          position: 'absolute',
          top: 2,
          left: on ? 18 : 2,
          width: 14,
          height: 14,
          borderRadius: '50%',
          background: on ? 'var(--interactive-accent-ink)' : 'var(--text-muted)',
          transition: 'left 120ms',
        }}
      />
    </button>
  )
}

export function StubRow({
  title,
  description,
  control,
}: {
  title: string
  description: string
  control: React.ReactNode
}) {
  return (
    <div className="settings-field">
      <div className="settings-field-header">
        <div className="settings-field-title">{title}</div>
        <div className="settings-field-control">{control}</div>
      </div>
      <div className="settings-field-description">{description}</div>
    </div>
  )
}

// ─── Wired primitives (P4-06) ────────────────────────────────────────────────
//
// Sibling components to the Stub* primitives above, but with their value
// hooked into the per-forge `configStore`. Use these for controls whose
// state we want to round-trip to `<forge>/.forge/app.toml` even if a
// real backend consumer doesn't exist yet — saving the value at least
// makes the UI feel honest, and future feature code can read the same
// key once the corresponding behavior ships.

export function WiredToggle({
  settingKey,
  defaultValue,
  label,
}: {
  settingKey: string
  defaultValue: boolean
  label: string
}) {
  const value = useConfigValue<boolean>(settingKey, defaultValue)
  const onClick = () => useConfigStore.getState().set(settingKey, !value)
  return <StubToggle on={value} label={label} onClick={onClick} />
}

export function WiredSelect({
  settingKey,
  defaultValue,
  options,
  label,
}: {
  settingKey: string
  defaultValue: string
  options: ReadonlyArray<{ value: string; label: string }>
  label: string
}) {
  const value = useConfigValue<string>(settingKey, defaultValue)
  return (
    <select
      value={value}
      aria-label={label}
      onChange={(e) => useConfigStore.getState().set(settingKey, e.target.value)}
    >
      {options.map((opt) => (
        <option key={opt.value} value={opt.value}>
          {opt.label}
        </option>
      ))}
    </select>
  )
}

export function WiredNumberRange({
  settingKey,
  defaultValue,
  min,
  max,
  step,
  label,
}: {
  settingKey: string
  defaultValue: number
  min: number
  max: number
  step?: number
  label: string
}) {
  const value = useConfigValue<number>(settingKey, defaultValue)
  return (
    <input
      type="range"
      min={min}
      max={max}
      step={step ?? 1}
      value={value}
      aria-label={label}
      onChange={(e) =>
        useConfigStore.getState().set(settingKey, Number(e.target.value))
      }
      style={{ minWidth: 120 }}
    />
  )
}

export function WiredText({
  settingKey,
  defaultValue,
  label,
  placeholder,
}: {
  settingKey: string
  defaultValue: string
  label: string
  placeholder?: string
}) {
  const value = useConfigValue<string>(settingKey, defaultValue)
  return (
    <input
      type="text"
      value={value}
      placeholder={placeholder}
      aria-label={label}
      onChange={(e) => useConfigStore.getState().set(settingKey, e.target.value)}
    />
  )
}

export function WiredNumber({
  settingKey,
  defaultValue,
  min,
  max,
  label,
}: {
  settingKey: string
  defaultValue: number
  min?: number
  max?: number
  label: string
}) {
  const value = useConfigValue<number>(settingKey, defaultValue)
  return (
    <input
      type="number"
      value={value}
      min={min}
      max={max}
      aria-label={label}
      onChange={(e) => useConfigStore.getState().set(settingKey, Number(e.target.value))}
    />
  )
}

export function CustomAppIconChooser({ api }: { api?: PluginAPI }) {
  const SETTING_KEY = 'nexus.settings.appearance.customAppIcon'
  const current = useConfigValue<string>(SETTING_KEY, '')
  const onPick = async () => {
    try {
      const picked = await openDialog({
        multiple: false,
        directory: false,
        filters: [{ name: 'Image', extensions: ['png', 'ico', 'icns', 'svg', 'jpg', 'jpeg'] }],
      })
      if (typeof picked === 'string' && picked.length > 0) {
        useConfigStore.getState().set(SETTING_KEY, picked)
        api?.notifications.show({ type: 'info', message: `Icon set: ${picked}` })
      }
    } catch (err) {
      api?.notifications.show({
        type: 'error',
        message: `Choose icon failed: ${err instanceof Error ? err.message : String(err)}`,
      })
    }
  }
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
      {current && (
        <span
          style={{
            fontSize: 11,
            color: 'var(--text-muted)',
            maxWidth: 220,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
          title={current}
        >
          {current.slice(current.lastIndexOf('/') + 1)}
        </span>
      )}
      <button
        type="button"
        onClick={onPick}
        style={{
          background: 'var(--background-modifier-hover)',
          color: 'var(--text-normal)',
          border: 'none',
          borderRadius: 4,
          padding: '4px 12px',
          fontSize: 13,
          cursor: 'pointer',
        }}
      >
        Choose
      </button>
      {current && (
        <button
          type="button"
          onClick={() => useConfigStore.getState().set(SETTING_KEY, '')}
          style={{
            background: 'transparent',
            color: 'var(--text-faint)',
            border: 'none',
            cursor: 'pointer',
            fontSize: 12,
          }}
          title="Clear"
        >
          ×
        </button>
      )}
    </div>
  )
}

export function WiredAccentColor({ settingKey }: { settingKey: string }) {
  const value = useConfigValue<string>(settingKey, '#8b5cf6')
  return (
    <input
      type="color"
      value={value}
      aria-label="Accent color"
      onChange={(e) => useConfigStore.getState().set(settingKey, e.target.value)}
      style={{
        width: 28,
        height: 28,
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 4,
        cursor: 'pointer',
        padding: 0,
        background: 'transparent',
      }}
    />
  )
}
