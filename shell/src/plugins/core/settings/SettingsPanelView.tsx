// src/plugins/core/settings/SettingsPanelView.tsx
// Auto-generates settings UI from registered config schemas. Plugin
// management (enable/disable, capability review, install-folder
// discovery) lives in the Plugins modal (`nexus.pluginsMgmt`); this
// file only renders the built-in tabs and per-plugin schema sections.

import { useState, useEffect, useRef, useCallback, useMemo, createElement, type MouseEvent as ReactMouseEvent } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { getRegistry } from '../../../host/shellRegistry'
import { useContextKey, useContextKeyStore } from '../../../host/ContextKeyService'
import { useConfigStore, useConfigValue } from '../../../stores/configStore'
import {
  useThemeStore,
  type AvailableSnippet,
} from '../../../stores/themeStore'
import type { ConfigSection, ConfigSchema, PluginAPI, SettingsTabEntry } from '../../../types/plugin'
import { eventBus } from '../../../host/EventBus'
import { clientLogger } from '../../../clientLogger'
import {
  formatChord,
  normalizeChord,
  type BindingRow,
} from '../../../registry/KeybindingRegistry'
import type { SnippetEntry, SnippetConflict } from '../../../registry/SnippetRegistry'

// ─── Data hooks ───────────────────────────────────────────────────────────────

function useConfigSections(): ConfigSection[] {
  const [sections, setSections] = useState<ConfigSection[]>([])
  const shellReady = useContextKey('shellReady')

  useEffect(() => {
    const reg = getRegistry()
    if (reg) setSections(reg.config.all())
  }, [shellReady])

  return sections
}

// Plugin-contributed Settings tabs (OI-01 + OI-08). Reads
// `SettingsTabRegistry.all()` (only tabs whose plugin has called
// `api.settings.registerTab` are returned, sorted by group/priority/id)
// and re-reads whenever a plugin registers/unregisters a tab or
// activates/deactivates so a hot-enabled plugin's tab appears
// without a reload.
function useContributedSettingsTabs(): SettingsTabEntry[] {
  const [tabs, setTabs] = useState<SettingsTabEntry[]>(() => {
    // Seed synchronously on first render so a plugin that registered
    // its tab BEFORE the settings panel mounted is visible on first
    // paint without waiting for the next event tick.
    const reg = getRegistry()
    return reg ? reg.settingsTabs.all() : []
  })
  const shellReady = useContextKey('shellReady')

  useEffect(() => {
    const reg = getRegistry()
    if (!reg) return
    const read = () => setTabs(reg.settingsTabs.all())
    read()
    // `settings:tabsChanged` covers the registerTab() path directly —
    // closes a race where a plugin activates, registers its tab, and
    // emits `plugin:activated` all in the same tick before this
    // effect subscribed. `plugin:activated` / `plugin:deactivated`
    // remain subscribed so a plugin that registers its tab from a
    // delayed code path (e.g. on first command invocation) is still
    // picked up.
    const offTabs = eventBus.on('settings:tabsChanged', read)
    const offActivated = eventBus.on('plugin:activated', read)
    const offDeactivated = eventBus.on('plugin:deactivated', read)
    return () => {
      offTabs()
      offActivated()
      offDeactivated()
    }
  }, [shellReady])

  return tabs
}

// ─── Main panel ───────────────────────────────────────────────────────────────

// Built-in rail entry ids. Plugin-contributed tab ids and per-plugin
// schema sections (rendered under the "Core plugins" group header) are
// opaque strings. Naming follows Obsidian for parity:
//   general       → about page (version + repo link)
//   appearance    → theme + snippets
//   hotkeys       → keybindings table (was 'keybindings')
//   snippets      → CSS snippets manager
//
// Plugin management lives in `nexus.pluginsMgmt` (Ctrl+Shift+X). The
// retired `'plugins'` and `'community-plugins'` ids are filtered out
// on load by the `BUILT_IN_TABS.includes` check below — sessions that
// last opened one of those tabs simply fall back to 'general'.
const BUILT_IN_TABS = [
  'general',
  'editor-options',
  'files-links',
  'appearance',
  'hotkeys',
  'keychain',
  'snippets',
] as const
type BuiltInTab = (typeof BUILT_IN_TABS)[number]
type NavTab = BuiltInTab | string

// Storage key for the last-opened tab. `api.storage.set` namespaces
// writes under `plugin:<id>:...` so this key resolves to
// `plugin:core.settings:last-tab` — same scheme as keybinding overrides.
const LAST_TAB_STORAGE_KEY = 'plugin:core.settings:last-tab'
const PANEL_OFFSET_STORAGE_KEY = 'plugin:core.settings:panel-offset'

// `api` is supplied by the settings plugin's `views.register()` wrapper
// in `index.ts` — the slot system itself doesn't pass props, so we
// inject it via a closure component there. Optional here because the
// Appearance tab is the only consumer; the other tabs reach the
// registry directly. When `api` is undefined the Appearance tab still
// renders but mutating actions are disabled.
interface SettingsPanelViewProps {
  api?: PluginAPI
}

export function SettingsPanelView(props: SettingsPanelViewProps = {}) {
  const { api } = props
  const visible    = useContextKey('settingsPanelVisible') as boolean
  const requestedTab = useContextKey('settingsActiveTab') as NavTab | undefined
  const sections   = useConfigSections()
  const contributedTabs = useContributedSettingsTabs()

  const [navTab, setNavTab] = useState<NavTab>('general')
  const [query, setQuery] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)

  // Drag state — translates the panel from its centered resting
  // position. Persisted so the user's chosen spot survives close/open.
  const [offset, setOffset] = useState<{ x: number; y: number }>(() => {
    try {
      const raw = localStorage.getItem(PANEL_OFFSET_STORAGE_KEY)
      if (!raw) return { x: 0, y: 0 }
      const parsed = JSON.parse(raw) as { x?: unknown; y?: unknown }
      const x = typeof parsed.x === 'number' ? parsed.x : 0
      const y = typeof parsed.y === 'number' ? parsed.y : 0
      return { x, y }
    } catch {
      return { x: 0, y: 0 }
    }
  })
  const [dragging, setDragging] = useState(false)
  const dragStartRef = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null)

  const onDragStart = useCallback((e: ReactMouseEvent) => {
    // Ignore drags that originate on interactive children — the input
    // needs to receive its own pointer events, the close button needs
    // its click. Only the bare topbar area initiates a drag.
    const target = e.target as HTMLElement
    if (target.closest('input, button, select, textarea, a')) return
    e.preventDefault()
    dragStartRef.current = { x: e.clientX, y: e.clientY, ox: offset.x, oy: offset.y }
    setDragging(true)
  }, [offset.x, offset.y])

  useEffect(() => {
    if (!dragging) return
    const onMove = (e: MouseEvent) => {
      const start = dragStartRef.current
      if (!start) return
      // Clamp so the panel can't be dragged entirely off-screen.
      // 40px of the panel must remain in the viewport on every edge.
      const maxX = Math.max(0, window.innerWidth / 2 - 40)
      const maxY = Math.max(0, window.innerHeight / 2 - 40)
      const nx = Math.max(-maxX, Math.min(maxX, start.ox + (e.clientX - start.x)))
      const ny = Math.max(-maxY, Math.min(maxY, start.oy + (e.clientY - start.y)))
      setOffset({ x: nx, y: ny })
    }
    const onUp = () => {
      setDragging(false)
      dragStartRef.current = null
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
    return () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
  }, [dragging])

  useEffect(() => {
    try {
      localStorage.setItem(PANEL_OFFSET_STORAGE_KEY, JSON.stringify(offset))
    } catch {
      // storage may be unavailable in tests — non-fatal.
    }
  }, [offset])

  // Hydrate the last-opened tab the first time the panel opens. The
  // panel mounts with the overlay at boot (hidden until `visible`
  // flips), so reading from storage inside a one-shot effect tied to
  // `visible` keeps the read off the hot path.
  const hydratedRef = useRef(false)
  useEffect(() => {
    if (!visible || hydratedRef.current) return
    hydratedRef.current = true
    try {
      const stored = localStorage.getItem(LAST_TAB_STORAGE_KEY)
      // Reject anything that isn't one of the four built-in tabs.
      // Older sessions may have persisted an `auto:` / `stub:` plugin
      // tab id; those rail entries no longer exist, so fall back to
      // 'settings' rather than landing on an empty "Unknown tab" body.
      if (stored && BUILT_IN_TABS.includes(stored as BuiltInTab)) {
        setNavTab(stored)
      }
    } catch {
      // localStorage may be unavailable in headless tests — swallow.
    }
  }, [visible])

  // Honour `settingsActiveTab` context key set by openKeybindings command.
  useEffect(() => {
    if (visible && requestedTab) {
      setNavTab(requestedTab)
      // Clear the request so subsequent opens don't re-route.
      useContextKeyStore.getState().set('settingsActiveTab', undefined)
    }
  }, [visible, requestedTab])

  // External "jump to plugin section" hook — used by AI chat's
  // empty-state CTA to land the user directly in nexus.ai's settings
  // group rather than whichever section was last open. The plugin id
  // is itself a navTab now (one rail entry per plugin schema).
  useEffect(() => {
    return eventBus.on('settings:focusSection', (pluginId: unknown) => {
      if (typeof pluginId !== 'string') return
      setNavTab(pluginId)
    })
  }, [])

  // Persist the active tab so the next open lands on the same page.
  useEffect(() => {
    try {
      localStorage.setItem(LAST_TAB_STORAGE_KEY, navTab)
    } catch {
      // See comment above — storage failures are non-fatal.
    }
  }, [navTab])

  const close = () => {
    useContextKeyStore.getState().set('settingsPanelVisible', false)
    setQuery('')
  }

  useEffect(() => {
    if (visible) setTimeout(() => inputRef.current?.focus(), 0)
  }, [visible])

  if (!visible) return null

  // Cross-plugin search across all registered schemas. Active only when
  // the search box has a query — overrides the rail's selected page.
  const searchHits = query
    ? sections
        .map((s) => ({
          ...s,
          schema: s.schema.filter(
            (f) =>
              f.title.toLowerCase().includes(query.toLowerCase()) ||
              f.description.toLowerCase().includes(query.toLowerCase()) ||
              f.key.toLowerCase().includes(query.toLowerCase()),
          ),
        }))
        .filter((s) => s.schema.length > 0)
    : []

  const sectionsByPlugin = new Map(sections.map((s) => [s.pluginId, s]))
  const optionsContributed = contributedTabs.filter(
    (t) => (t.group ?? 'options') === 'options',
  )
  const pluginContributed = contributedTabs.filter((t) => (t.group ?? 'options') !== 'options')

  return (
    <div
      className="settings-backdrop"
      onClick={close}
      style={{ pointerEvents: 'auto' }}
    >
      <div
        className="settings-panel"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === 'Escape' && close()}
        style={{
          transform: `translate(${offset.x}px, ${offset.y}px)`,
          transition: dragging ? 'none' : 'transform 120ms ease-out',
        }}
      >
        {/* Left rail — Obsidian-style flat nav with grouped sections.
            "Options" lists the built-in pages; "Core plugins" lists every
            plugin schema as its own page. No inner sub-nav. */}
        <nav
          className={`settings-rail settings-rail--drag${dragging ? ' settings-rail--dragging' : ''}`}
          onMouseDown={onDragStart}
        >
          <div className="settings-rail-group-header">Options</div>
          <RailItem
            label="General"
            active={navTab === 'general'}
            onClick={() => setNavTab('general')}
          />
          <RailItem
            label="Editor"
            active={navTab === 'editor-options'}
            onClick={() => setNavTab('editor-options')}
          />
          <RailItem
            label="Files and links"
            active={navTab === 'files-links'}
            onClick={() => setNavTab('files-links')}
          />
          <RailItem
            label="Appearance"
            active={navTab === 'appearance'}
            onClick={() => setNavTab('appearance')}
          />
          <RailItem
            label="Hotkeys"
            active={navTab === 'hotkeys'}
            onClick={() => setNavTab('hotkeys')}
          />
          <RailItem
            label="Keychain"
            active={navTab === 'keychain'}
            onClick={() => setNavTab('keychain')}
          />
          {/* Plugin management lives in the standalone Plugins modal
              (`nexus.pluginsMgmt`). The rail entry is an action, not a
              nav target — clicking it closes the settings panel and
              opens the modal so the user has a discoverable path
              from Settings into plugin enable/disable + capability
              review without needing to know the Ctrl+Shift+X shortcut. */}
          <RailItem
            label="Plugins…"
            active={false}
            title="Open the Plugins panel (Ctrl+Shift+X)"
            onClick={() => {
              useContextKeyStore.getState().set('settingsPanelVisible', false)
              const reg = getRegistry()
              if (reg) void reg.commands.execute('nexus.plugins.open')
            }}
          />
          <RailItem
            label="Snippets"
            active={navTab === 'snippets'}
            onClick={() => setNavTab('snippets')}
          />
          {optionsContributed.map((t) => (
            <RailItem
              key={t.id}
              label={t.title}
              active={navTab === t.id}
              onClick={() => setNavTab(t.id)}
            />
          ))}

          {(sections.length > 0 || STUB_CORE_PLUGINS.length > 0) && (
            <div className="settings-rail-group-header">Core plugins</div>
          )}
          {sections.map((s) => (
            <RailItem
              key={s.pluginId}
              label={s.title}
              active={navTab === s.pluginId}
              onClick={() => setNavTab(s.pluginId)}
            />
          ))}
          {STUB_CORE_PLUGINS.map((p) => (
            <RailItem
              key={p.id}
              label={p.label}
              active={navTab === p.id}
              onClick={() => setNavTab(p.id)}
            />
          ))}

          {pluginContributed.length > 0 && (
            <div className="settings-rail-group-header">Community plugins</div>
          )}
          {pluginContributed.map((t) => (
            <RailItem
              key={t.id}
              label={t.title}
              active={navTab === t.id}
              onClick={() => setNavTab(t.id)}
            />
          ))}
        </nav>

        {/* Right pane — topbar + content for the selected rail entry.
            Search lives in the topbar regardless of tab; an active query
            overrides the page body with cross-plugin search results. */}
        <div className="settings-main">
          <div
            className={`settings-topbar settings-topbar--drag${dragging ? ' settings-topbar--dragging' : ''}`}
            onMouseDown={onDragStart}
          >
            <input
              ref={inputRef}
              className="settings-search"
              placeholder="Search settings..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <button
              className="settings-close"
              title="Edit settings file (.forge/app.toml)"
              aria-label="Edit settings file"
              onClick={async () => {
                // The config layer is lazy: `app.toml` only exists once
                // a setting has been written. Opening a non-existent
                // file in the editor would crash session.acquire — so
                // seed a minimal stub if it's missing before routing
                // through `files:open`.
                try {
                  const probe = await invoke<{ bytes: number[] | null }>('kernel_invoke', {
                    pluginId: 'com.nexus.storage',
                    commandId: 'read_file',
                    args: { path: '.forge/app.toml' },
                    timeoutMs: null,
                  })
                  if (probe.bytes === null) {
                    const stub = '# Forge settings (.forge/app.toml)\n\n[settings]\n'
                    await invoke('kernel_invoke', {
                      pluginId: 'com.nexus.storage',
                      commandId: 'write_file',
                      args: {
                        path: '.forge/app.toml',
                        bytes: Array.from(new TextEncoder().encode(stub)),
                      },
                      timeoutMs: null,
                    })
                  }
                } catch (err) {
                  // Non-fatal: the editor's session manager will log
                  // and degrade gracefully if the file still isn't
                  // openable after this best-effort seed.
                  clientLogger.warn('[settings] ensure app.toml failed', err)
                }
                eventBus.emit('files:open', {
                  relpath: '.forge/app.toml',
                  name: 'app.toml',
                })
                close()
              }}
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                <polyline points="14 2 14 8 20 8" />
                <line x1="9" y1="13" x2="15" y2="13" />
                <line x1="9" y1="17" x2="13" y2="17" />
              </svg>
            </button>
            <button className="settings-close" onClick={close}>✕</button>
          </div>

          <div className="settings-body">
            <div className="settings-content">
              {query ? (
                <>
                  {searchHits.length === 0 && (
                    <p className="settings-empty">
                      No settings found for &ldquo;{query}&rdquo;
                    </p>
                  )}
                  {searchHits.map((section) => (
                    <SettingsSection key={section.pluginId} section={section} />
                  ))}
                </>
              ) : navTab === 'general' ? (
                <GeneralTab />
              ) : navTab === 'editor-options' ? (
                <EditorOptionsTab />
              ) : navTab === 'files-links' ? (
                <FilesLinksTab api={api} />
              ) : navTab === 'appearance' ? (
                <AppearanceTab api={api} />
              ) : navTab === 'hotkeys' ? (
                <KeybindingsTab />
              ) : navTab === 'keychain' ? (
                <KeychainTab api={api} />
              ) : navTab === 'snippets' ? (
                <SnippetsTab />
              ) : sectionsByPlugin.has(navTab) ? (
                <SettingsSection section={sectionsByPlugin.get(navTab)!} />
              ) : STUB_CORE_BY_ID.has(navTab) ? (
                STUB_CORE_BY_ID.get(navTab)!.render(api)
              ) : (
                <ContributedTabBody navTab={navTab} />
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

// ─── General page ─────────────────────────────────────────────────────────────
//
// Lightweight "About" landing page. Mirrors Obsidian's General > Version
// block but skips the auto-update / language / commercial-license rows
// since none of those apply yet.

// ─── Stub-page primitives ─────────────────────────────────────────────────────
//
// Shared pieces for the settings pages. Every control that had a real
// value to persist has been migrated to the `Wired*` primitives below;
// the few remaining bespoke buttons (Sync sign-up, Help, Custom app icon,
// etc.) call into Tauri/configStore directly inline.

function StubToggle({
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

function StubRow({
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

function WiredToggle({
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

function WiredSelect({
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

function WiredNumberRange({
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

function WiredText({
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

function WiredNumber({
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

function CustomAppIconChooser({ api }: { api?: PluginAPI }) {
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

function WiredAccentColor({ settingKey }: { settingKey: string }) {
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

function GeneralTab() {
  const version = (import.meta.env?.VITE_APP_VERSION as string | undefined) ?? '0.1.0'

  return (
    <div className="settings-section">
      <div className="settings-section-title">About Nexus</div>

      <div className="settings-field">
        <div className="settings-field-header">
          <div className="settings-field-title">Version</div>
        </div>
        <div className="settings-field-description">{version}</div>
      </div>

      <div className="settings-field">
        <div className="settings-field-header">
          <div className="settings-field-title">Source</div>
        </div>
        <div className="settings-field-description">
          <a
            href="https://github.com/baileyrd/nexus"
            target="_blank"
            rel="noreferrer"
            onClick={(e) => {
              e.preventDefault()
              window.open('https://github.com/baileyrd/nexus', '_blank')
            }}
          >
            github.com/baileyrd/nexus
          </a>
        </div>
      </div>

      <StubRow
        title="Automatic updates"
        description="Turn this off to prevent Nexus from checking for updates."
        control={
          <WiredToggle
            settingKey="nexus.settings.general.automaticUpdates"
            defaultValue={true}
            label="Toggle automatic updates"
          />
        }
      />

      <StubRow
        title="Language"
        description="Change the display language."
        control={
          <WiredSelect
            settingKey="nexus.settings.general.language"
            defaultValue="en"
            label="Language"
            options={[{ value: 'en', label: 'English' }]}
          />
        }
      />

      <StubRow
        title="Help"
        description="Learn how to use Nexus and get help from the community."
        control={
          <button
            type="button"
            onClick={() =>
              window.open('https://github.com/baileyrd/nexus#readme', '_blank')
            }
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
            Open
          </button>
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>
        Advanced
      </div>

      <StubRow
        title="Notify if startup takes longer than expected"
        description="Diagnose issues by seeing what is causing the app to load slowly."
        control={
          <WiredToggle
            settingKey="nexus.settings.general.slowStartupNotification"
            defaultValue={false}
            label="Toggle slow-startup notification"
          />
        }
      />

      <StubRow
        title="Command line interface"
        description="Allow interactions with Nexus from the command line."
        control={
          <WiredToggle
            settingKey="nexus.settings.general.commandLineInterface"
            defaultValue={false}
            label="Toggle command line interface"
          />
        }
      />
    </div>
  )
}

// ─── Editor page (stub) ──────────────────────────────────────────────────────
//
// Mirrors Obsidian's Editor settings — same row order and labels. None
// of these toggles are wired to real preferences yet; they render in
// their Obsidian default state and surface a "Coming soon" toast on
// interaction. Real per-plugin editor settings already live under
// `Core plugins > nexus.editor`; this stub will eventually consolidate
// them into a single Obsidian-style page.

function EditorOptionsTab() {
  return (
    <div className="settings-section">
      <StubRow
        title="Always focus new tabs"
        description="When you open a link in a new tab, switch to it immediately."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.alwaysFocusNewTabs"
            defaultValue={true}
            label="Toggle focus new tabs"
          />
        }
      />
      <StubRow
        title="Default view for new tabs"
        description="The default view that a new Markdown tab gets opened in."
        control={
          <WiredSelect
            settingKey="nexus.settings.editor.defaultView"
            defaultValue="editing"
            label="Default view for new tabs"
            options={[
              { value: 'editing', label: 'Editing view' },
              { value: 'reading', label: 'Reading view' },
            ]}
          />
        }
      />
      <StubRow
        title="Default editing mode"
        description="The default editing mode a new tab will start with."
        control={
          <WiredSelect
            settingKey="nexus.settings.editor.defaultEditingMode"
            defaultValue="live"
            label="Default editing mode"
            options={[
              { value: 'live', label: 'Live Preview' },
              { value: 'source', label: 'Source mode' },
            ]}
          />
        }
      />
      <StubRow
        title="Show editing mode in status bar"
        description="Show the editing mode toggle in the status bar."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.showEditingModeInStatusBar"
            defaultValue={true}
            label="Toggle editing-mode status bar"
          />
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Display</div>

      <StubRow
        title="Readable line length"
        description="Limit maximum line length. Less content fits onscreen, but long blocks of text are more readable."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.readableLineLength"
            defaultValue={true}
            label="Toggle readable line length"
          />
        }
      />
      <StubRow
        title="Strict line breaks"
        description="Markdown specs ignore single line breaks in reading view. Turn this off to make single line breaks visible."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.strictLineBreaks"
            defaultValue={false}
            label="Toggle strict line breaks"
          />
        }
      />
      <StubRow
        title="Properties in document"
        description="Choose how properties are displayed at the top of notes. Select &ldquo;source&rdquo; to show properties as raw YAML."
        control={
          <WiredSelect
            settingKey="nexus.settings.editor.propertiesInDocument"
            defaultValue="visible"
            label="Properties in document"
            options={[
              { value: 'visible', label: 'Visible' },
              { value: 'hidden', label: 'Hidden' },
              { value: 'source', label: 'Source' },
            ]}
          />
        }
      />
      <StubRow
        title="Fold heading"
        description="Lets you fold all content under a heading."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.foldHeading"
            defaultValue={true}
            label="Toggle fold heading"
          />
        }
      />
      <StubRow
        title="Fold indent"
        description="Lets you fold part of an indentation, such as lists."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.foldIndent"
            defaultValue={true}
            label="Toggle fold indent"
          />
        }
      />
      <StubRow
        title="Line numbers"
        description="Show line numbers in the gutter."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.lineNumbers"
            defaultValue={false}
            label="Toggle line numbers"
          />
        }
      />
      <StubRow
        title="Indentation guides"
        description="Show vertical relationship lines between list items."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.indentationGuides"
            defaultValue={true}
            label="Toggle indentation guides"
          />
        }
      />
      <StubRow
        title="Right-to-left (RTL)"
        description="Sets the default text direction of notes to right-to-left."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.rtl"
            defaultValue={false}
            label="Toggle RTL"
          />
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Behavior</div>

      <StubRow
        title="Spellcheck"
        description="Turn on the spellchecker."
        control={
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <WiredToggle
              settingKey="nexus.settings.editor.spellcheck"
              defaultValue={true}
              label="Toggle spellcheck"
            />
          </div>
        }
      />
      <StubRow
        title="Spellcheck languages"
        description="Choose the languages for the spellchecker to use."
        control={
          <WiredSelect
            settingKey="nexus.settings.editor.spellcheckLanguages"
            defaultValue="en-US"
            label="Spellcheck languages"
            options={[
              { value: 'en-US', label: 'English (United States)' },
              { value: 'add', label: '+ Add language…' },
            ]}
          />
        }
      />
      <StubRow
        title="Auto-pair brackets"
        description="Pair brackets and quotes automatically."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.autoPairBrackets"
            defaultValue={true}
            label="Toggle auto-pair brackets"
          />
        }
      />
      <StubRow
        title="Auto-pair Markdown syntax"
        description="Pair symbols automatically for bold, italic, code, and more."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.autoPairMarkdownSyntax"
            defaultValue={true}
            label="Toggle auto-pair Markdown syntax"
          />
        }
      />
      <StubRow
        title="Smart lists"
        description="Automatically set indentation and place list items correctly."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.smartLists"
            defaultValue={true}
            label="Toggle smart lists"
          />
        }
      />
      <StubRow
        title="Indent using tabs"
        description="Use tabs to indent by pressing the &ldquo;Tab&rdquo; key. Turn this off to indent using 4 spaces."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.indentUsingTabs"
            defaultValue={true}
            label="Toggle indent using tabs"
          />
        }
      />
      <StubRow
        title="Indent visual width"
        description="Number of spaces a tab character will render as."
        control={
          <WiredNumberRange
            settingKey="nexus.settings.editor.indentVisualWidth"
            defaultValue={4}
            min={2}
            max={8}
            label="Indent visual width"
          />
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Advanced</div>

      <StubRow
        title="Convert pasted HTML to Markdown"
        description="Automatically convert HTML to Markdown when pasting and drag-and-drop from web pages. Use Ctrl/Cmd+Shift+V to paste HTML without converting."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.convertPastedHtml"
            defaultValue={true}
            label="Toggle convert pasted HTML"
          />
        }
      />
      <StubRow
        title="Vim key bindings"
        description="Use Vim key bindings when editing."
        control={
          <WiredToggle
            settingKey="nexus.settings.editor.vimKeyBindings"
            defaultValue={false}
            label="Toggle Vim key bindings"
          />
        }
      />
    </div>
  )
}

// ─── Files and links page (stub) ─────────────────────────────────────────────
//
// Mirrors Obsidian's Files and links settings. None of these are wired
// to real preferences yet; controls render in their Obsidian default
// state and surface a "Coming soon" toast on interaction. Terminology
// updated to forge/Nexus where applicable.

function FilesLinksTab({ api }: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Default file to open"
        description="Choose which file to open when the app starts."
        control={
          <WiredSelect
            settingKey="nexus.settings.files.defaultFileToOpen"
            defaultValue="last"
            label="Default file to open"
            options={[
              { value: 'last', label: 'Last opened' },
              { value: 'none', label: 'None' },
              { value: 'specific', label: 'Specific file…' },
            ]}
          />
        }
      />
      <StubRow
        title="Default location for new notes"
        description="Where newly created notes are placed."
        control={
          <WiredSelect
            settingKey="nexus.settings.files.defaultNoteLocation"
            defaultValue="root"
            label="Default location for new notes"
            options={[
              { value: 'root', label: 'Forge folder' },
              { value: 'same', label: 'Same folder as current file' },
              { value: 'specific', label: 'Specific folder…' },
            ]}
          />
        }
      />
      <StubRow
        title="Default location for new attachments"
        description="Where newly added attachments are placed."
        control={
          <WiredSelect
            settingKey="nexus.settings.files.defaultAttachmentLocation"
            defaultValue="root"
            label="Default location for new attachments"
            options={[
              { value: 'root', label: 'Forge folder' },
              { value: 'same', label: 'Same folder as current file' },
              { value: 'specific', label: 'Specific folder…' },
            ]}
          />
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Links</div>

      <StubRow
        title="New link format"
        description="What links to insert when auto-generating internal links."
        control={
          <WiredSelect
            settingKey="nexus.settings.links.newLinkFormat"
            defaultValue="shortest"
            label="New link format"
            options={[
              { value: 'shortest', label: 'Shortest path when possible' },
              { value: 'relative', label: 'Relative path' },
              { value: 'absolute', label: 'Absolute path' },
            ]}
          />
        }
      />
      <StubRow
        title="Automatically update internal links"
        description="Turn off to be prompted to update links after renaming a file."
        control={
          <WiredToggle
            settingKey="nexus.settings.links.autoUpdate"
            defaultValue={false}
            label="Toggle automatic link updates"
          />
        }
      />
      <StubRow
        title="Use [[Wikilinks]]"
        description="Auto-generate Wikilinks for [[links]] and ![[images]] instead of Markdown links and images. Disable this option to generate Markdown links instead."
        control={
          <WiredToggle
            settingKey="nexus.settings.links.useWikilinks"
            defaultValue={true}
            label="Toggle wikilinks"
          />
        }
      />
      <StubRow
        title="Show all file types"
        description="Show files with any extension even if Nexus can't open them natively, so you can link to them and see them in the file explorer and quick switcher."
        control={
          <WiredToggle
            settingKey="nexus.settings.files.showAllFileTypes"
            defaultValue={false}
            label="Toggle show all file types"
          />
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Trash</div>

      <StubRow
        title="Confirm before deleting files"
        description="Avoid accidentally deleting files."
        control={
          <WiredToggle
            settingKey="nexus.settings.files.confirmBeforeDelete"
            defaultValue={true}
            label="Toggle delete confirmation"
          />
        }
      />
      <StubRow
        title="Delete attachments when deleting files"
        description="Automatically remove attachments linked to the deleted file if they're not used elsewhere."
        control={
          <WiredSelect
            settingKey="nexus.settings.files.deleteAttachments"
            defaultValue="ask"
            label="Delete attachments when deleting files"
            options={[
              { value: 'ask', label: 'Ask each time' },
              { value: 'always', label: 'Always' },
              { value: 'never', label: 'Never' },
            ]}
          />
        }
      />
      <StubRow
        title="Deleted files"
        description="What happens to a file after you delete it."
        control={
          <WiredSelect
            settingKey="nexus.settings.files.deletedFilesDestination"
            defaultValue="system"
            label="Deleted files"
            options={[
              { value: 'system', label: 'Move to system trash' },
              { value: 'forge', label: 'Move to .trash in forge' },
              { value: 'permanent', label: 'Delete permanently' },
            ]}
          />
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Advanced</div>

      <StubRow
        title="Excluded files"
        description="Excluded files will be hidden in Search, Graph view, and Unlinked Mentions, less noticeable in Quick Switcher and link suggestions. Comma-separated globs."
        control={
          <WiredText
            settingKey="nexus.settings.files.excludedPatterns"
            defaultValue=""
            placeholder=".obsidian/*, node_modules/*, *.tmp"
            label="Excluded files"
          />
        }
      />
      <StubRow
        title="Override config folder"
        description="Use a different config folder than the default one. Must start with a dot."
        control={
          <WiredText
            settingKey="nexus.settings.files.overrideConfigFolder"
            defaultValue=""
            placeholder=".forge"
            label="Override config folder"
          />
        }
      />
      <StubRow
        title="Allow URI callbacks"
        description="Enable the use of x-callback-url through x-success or x-error when handling Nexus URIs."
        control={
          <WiredToggle
            settingKey="nexus.settings.files.allowUriCallbacks"
            defaultValue={false}
            label="Toggle URI callbacks"
          />
        }
      />
      <StubRow
        title="Rebuild forge cache"
        description="Rebuilding the cache could take a few seconds to a few minutes depending on the size of your forge."
        control={
          <button
            type="button"
            onClick={async () => {
              try {
                await invoke('kernel_invoke', {
                  pluginId: 'com.nexus.storage',
                  commandId: 'rebuild_index',
                  args: {},
                  timeoutMs: null,
                })
                api?.notifications.show({
                  type: 'info',
                  message: 'Forge cache rebuilt.',
                })
              } catch (err) {
                api?.notifications.show({
                  type: 'error',
                  message: `Rebuild failed: ${err instanceof Error ? err.message : String(err)}`,
                })
              }
            }}
            style={{
              background: 'transparent',
              color: 'var(--text-error, #e06c75)',
              border: '1px solid var(--text-error, #e06c75)',
              borderRadius: 4,
              padding: '4px 12px',
              fontSize: 13,
              cursor: 'pointer',
            }}
          >
            Rebuild
          </button>
        }
      />
    </div>
  )
}

// ─── Keychain page (stub) ────────────────────────────────────────────────────
//
// Mirrors Obsidian's Keychain layout: "Secrets" header with a + add
// button, and an empty-state info card explaining what secrets are
// for. Adding a secret fires a "Coming soon" toast — wiring to the
// platform keyring is tracked separately.

function KeychainTab({ api }: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          marginBottom: 12,
        }}
      >
        <div className="settings-section-title" style={{ margin: 0 }}>Secrets</div>
        <button
          type="button"
          onClick={async () => {
            const pluginId = await api?.input.prompt(
              'Plugin id that will read this secret (e.g. com.nexus.ai):',
              'com.nexus.ai',
            )
            if (!pluginId) return
            const name = await api?.input.prompt(
              `Secret name for ${pluginId}:`,
              'api_key',
            )
            if (!name) return
            const value = await api?.input.prompt(
              `Value for ${pluginId}:${name}:`,
              '',
            )
            if (value === null || value === undefined) return
            try {
              await invoke('kernel_invoke', {
                pluginId: 'com.nexus.security',
                commandId: 'set_secret',
                args: { plugin_id: pluginId, name, value },
                timeoutMs: null,
              })
              api?.notifications.show({
                type: 'info',
                message: `Secret ${pluginId}:${name} stored.`,
              })
            } catch (err) {
              api?.notifications.show({
                type: 'error',
                message: `Add secret failed: ${err instanceof Error ? err.message : String(err)}`,
              })
            }
          }}
          aria-label="Add secret"
          style={{
            background: 'transparent',
            border: 'none',
            color: 'var(--text-muted)',
            cursor: 'pointer',
            fontSize: 18,
            lineHeight: 1,
            padding: '2px 6px',
            borderRadius: 4,
          }}
        >
          +
        </button>
      </div>
      <div
        style={{
          padding: '14px 16px',
          background: 'var(--background-modifier-hover)',
          color: 'var(--text-muted)',
          borderRadius: 6,
          fontSize: 13,
          lineHeight: 1.5,
        }}
      >
        No secrets have been added. Secrets are used to store information like API
        keys and passwords that plugins can use.
      </div>
    </div>
  )
}

// ─── Obsidian-parity core plugin stubs ──────────────────────────────────────
//
// Rail entries for ten Obsidian core plugins that don't exist in Nexus
// yet. Each opens a stub settings page that mirrors the Obsidian layout
// exactly — controls render in their default state and surface a
// "Coming soon" toast on interaction.
//
// Tab ids live in their own `cp-stub:<name>` namespace so they don't
// collide with real plugin ids. The list also drives the rail below
// the "Core plugins" header in alphabetical order.

interface StubCorePluginEntry {
  id: string
  label: string
  render: (api: PluginAPI | undefined) => React.ReactNode
}

const STUB_CORE_PLUGINS: ReadonlyArray<StubCorePluginEntry> = [
  {
    id: 'cp-stub:backlinks',
    label: 'Backlinks',
    render: (api) => <StubBacklinksPage api={api} />,
  },
  {
    id: 'cp-stub:canvas',
    label: 'Canvas',
    render: (api) => <StubCanvasPage api={api} />,
  },
  {
    id: 'cp-stub:command-palette',
    label: 'Command palette',
    render: () => <StubCommandPalettePage />,
  },
  {
    id: 'cp-stub:daily-notes',
    label: 'Daily notes',
    render: (api) => <StubDailyNotesPage api={api} />,
  },
  {
    id: 'cp-stub:file-recovery',
    label: 'File recovery',
    render: (api) => <StubFileRecoveryPage api={api} />,
  },
  {
    id: 'cp-stub:note-composer',
    label: 'Note composer',
    render: (api) => <StubNoteComposerPage api={api} />,
  },
  {
    id: 'cp-stub:page-preview',
    label: 'Page preview',
    render: (api) => <StubPagePreviewPage api={api} />,
  },
  {
    id: 'cp-stub:quick-switcher',
    label: 'Quick switcher',
    render: (api) => <StubQuickSwitcherPage api={api} />,
  },
  {
    id: 'cp-stub:sync',
    label: 'Sync',
    render: () => <StubSyncPage />,
  },
  {
    id: 'cp-stub:templates',
    label: 'Templates',
    render: (api) => <StubTemplatesPage api={api} />,
  },
]

const STUB_CORE_BY_ID = new Map(STUB_CORE_PLUGINS.map((p) => [p.id, p]))

function StubBacklinksPage(_: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Show backlinks at the bottom of notes"
        description="Make backlinks visible in new tabs by default."
        control={
          <WiredToggle
            settingKey="nexus.settings.backlinks.showAtBottom"
            defaultValue={false}
            label="Toggle backlinks at bottom"
          />
        }
      />
    </div>
  )
}

function StubCanvasPage(_: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Default location for new canvas files"
        description="Where newly created canvases are placed."
        control={
          <WiredSelect
            settingKey="nexus.settings.canvas.defaultLocation"
            defaultValue="root"
            label="Default canvas location"
            options={[
              { value: 'root', label: 'Forge folder' },
              { value: 'same', label: 'Same folder as current file' },
              { value: 'specific', label: 'Specific folder…' },
            ]}
          />
        }
      />
      <StubRow
        title="Default mouse wheel behavior"
        description=""
        control={
          <WiredSelect
            settingKey="nexus.settings.canvas.mouseWheel"
            defaultValue="pan"
            label="Default mouse wheel behavior"
            options={[
              { value: 'pan', label: 'Pan' },
              { value: 'zoom', label: 'Zoom' },
            ]}
          />
        }
      />
      <StubRow
        title="Default Ctrl + Drag behavior"
        description=""
        control={
          <WiredSelect
            settingKey="nexus.settings.canvas.ctrlDrag"
            defaultValue="menu"
            label="Default Ctrl+Drag behavior"
            options={[
              { value: 'menu', label: 'Show menu' },
              { value: 'select', label: 'Select' },
              { value: 'zoom', label: 'Zoom' },
            ]}
          />
        }
      />
      <StubRow
        title="Show card names"
        description=""
        control={
          <WiredSelect
            settingKey="nexus.settings.canvas.showCardNames"
            defaultValue="always"
            label="Show card names"
            options={[
              { value: 'always', label: 'Always' },
              { value: 'hover', label: 'On hover' },
              { value: 'never', label: 'Never' },
            ]}
          />
        }
      />
      <StubRow
        title="Snap to grid"
        description="Snap cards to the background grid when moving and resizing."
        control={
          <WiredToggle
            settingKey="nexus.settings.canvas.snapToGrid"
            defaultValue={true}
            label="Toggle snap to grid"
          />
        }
      />
      <StubRow
        title="Snap to objects"
        description="Snap cards to nearby objects when moving and resizing."
        control={
          <WiredToggle
            settingKey="nexus.settings.canvas.snapToObjects"
            defaultValue={true}
            label="Toggle snap to objects"
          />
        }
      />
      <StubRow
        title="Zoom threshold for hiding card content"
        description="Lower values will increase performance but hide card content sooner when zooming out."
        control={
          <WiredNumberRange
            settingKey="nexus.settings.canvas.zoomHideThreshold"
            defaultValue={40}
            min={0}
            max={100}
            label="Zoom threshold for hiding card content"
          />
        }
      />
    </div>
  )
}

function StubCommandPalettePage() {
  return (
    <div className="settings-section">
      <div className="settings-section-title">Pinned commands</div>
      <div
        style={{
          padding: '14px 16px',
          background: 'var(--background-modifier-hover)',
          borderRadius: 6,
        }}
      >
        <input
          type="search"
          className="settings-search"
          placeholder="Select a command to add..."
          disabled
          style={{ width: '100%', marginBottom: 8 }}
          title="Coming soon"
        />
        <div style={{ color: 'var(--text-muted)', fontSize: 13 }}>No commands found.</div>
      </div>
    </div>
  )
}

function StubDailyNotesPage(_: { api?: PluginAPI }) {
  const today = new Date().toISOString().slice(0, 10)
  return (
    <div className="settings-section">
      <StubRow
        title="Date format"
        description="Choose how daily notes are named in your forge."
        control={
          <WiredText
            settingKey="nexus.settings.dailyNotes.dateFormat"
            defaultValue={today}
            label="Date format"
          />
        }
      />
      <StubRow
        title="New file location"
        description="New daily notes will be placed here."
        control={
          <WiredText
            settingKey="nexus.settings.dailyNotes.fileLocation"
            defaultValue=""
            placeholder="Example: folder 1/folder 2"
            label="Daily note location"
          />
        }
      />
      <StubRow
        title="Template file location"
        description="Choose the file to use as a template."
        control={
          <WiredText
            settingKey="nexus.settings.dailyNotes.templateLocation"
            defaultValue=""
            placeholder="Example: folder/note"
            label="Daily note template"
          />
        }
      />
    </div>
  )
}

function StubFileRecoveryPage({ api }: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Snapshot interval"
        description="Minimal interval in minutes between two snapshots."
        control={
          <WiredNumber
            settingKey="nexus.settings.fileRecovery.snapshotIntervalMinutes"
            defaultValue={5}
            min={1}
            label="Snapshot interval"
          />
        }
      />
      <StubRow
        title="History length"
        description="Number of days the snapshots are kept for."
        control={
          <WiredNumber
            settingKey="nexus.settings.fileRecovery.historyDays"
            defaultValue={7}
            min={1}
            label="History length"
          />
        }
      />
      <StubRow
        title="Snapshots"
        description="View and restore saved snapshots. Until the file-recovery subsystem ships, the list is always empty — the values above are persisted so they take effect the moment snapshots come online."
        control={
          <button
            type="button"
            onClick={() =>
              api?.notifications.show({
                type: 'info',
                message: 'No snapshots — the file-recovery daemon is not yet built.',
              })
            }
            style={{
              background: 'var(--interactive-accent)',
              color: 'var(--interactive-accent-ink)',
              border: 'none',
              borderRadius: 4,
              padding: '4px 12px',
              fontSize: 13,
              cursor: 'pointer',
            }}
          >
            View
          </button>
        }
      />
      <StubRow
        title="Clear history"
        description="Delete all snapshots."
        control={
          <button
            type="button"
            onClick={() =>
              api?.notifications.show({
                type: 'info',
                message: 'Snapshot history is empty — nothing to clear.',
              })
            }
            style={{
              background: 'transparent',
              color: 'var(--text-error, #e06c75)',
              border: '1px solid var(--text-error, #e06c75)',
              borderRadius: 4,
              padding: '4px 12px',
              fontSize: 13,
              cursor: 'pointer',
            }}
          >
            Clear
          </button>
        }
      />
    </div>
  )
}

function StubNoteComposerPage(_: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Text after extraction"
        description="What to show in place of the selected text after extracting it."
        control={
          <WiredSelect
            settingKey="nexus.settings.noteComposer.textAfterExtraction"
            defaultValue="link"
            label="Text after extraction"
            options={[
              { value: 'link', label: 'Link to new file' },
              { value: 'embed', label: 'Embed new file' },
              { value: 'nothing', label: 'Nothing' },
            ]}
          />
        }
      />
      <StubRow
        title="Template file location"
        description="Template file to use when merging or extracting. Available variables: {{content}}, {{fromTitle}}, {{newTitle}}, {{date:FORMAT}}, e.g. {{date:YYYY-MM-DD}}."
        control={
          <WiredText
            settingKey="nexus.settings.noteComposer.templateLocation"
            defaultValue=""
            placeholder="Example: folder/note"
            label="Note composer template"
          />
        }
      />
      <StubRow
        title="Confirm file merge"
        description="Prompt before merging two files."
        control={
          <WiredToggle
            settingKey="nexus.settings.noteComposer.confirmMerge"
            defaultValue={true}
            label="Toggle confirm file merge"
          />
        }
      />
    </div>
  )
}

function StubPagePreviewPage(_: { api?: PluginAPI }) {
  const surfaces: ReadonlyArray<{ key: string; label: string; on: boolean }> = [
    { key: 'search', label: 'Search, Backlinks, and Outgoing links', on: true },
    { key: 'reading', label: 'Reading view', on: false },
    { key: 'editing', label: 'Editing view', on: true },
    { key: 'tabs', label: 'Tab header', on: true },
    { key: 'files', label: 'Files', on: true },
    { key: 'properties', label: 'Properties view', on: true },
    { key: 'bookmarks', label: 'Bookmarks', on: true },
    { key: 'outline', label: 'Outline', on: true },
    { key: 'bases', label: 'Bases', on: true },
    { key: 'graph', label: 'Graph view', on: true },
  ]
  return (
    <div className="settings-section">
      <div className="settings-section-title">Require Ctrl to trigger page preview on hover</div>
      {surfaces.map((s) => (
        <StubRow
          key={s.key}
          title={s.label}
          description=""
          control={
            <WiredToggle
              settingKey={`nexus.settings.pagePreview.ctrlRequired.${s.key}`}
              defaultValue={s.on}
              label={`Toggle Ctrl-required on ${s.label}`}
            />
          }
        />
      ))}
    </div>
  )
}

function StubQuickSwitcherPage(_: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Show existing only"
        description="Only show results from existing files. Links to files that are not yet created will be hidden."
        control={
          <WiredToggle
            settingKey="nexus.settings.quickSwitcher.showExistingOnly"
            defaultValue={false}
            label="Toggle show existing only"
          />
        }
      />
      <StubRow
        title="Show attachments"
        description="Show attachment files like images, videos, and PDFs."
        control={
          <WiredToggle
            settingKey="nexus.settings.quickSwitcher.showAttachments"
            defaultValue={true}
            label="Toggle show attachments"
          />
        }
      />
    </div>
  )
}

function StubSyncPage() {
  return (
    <div className="settings-section">
      <p style={{ marginBottom: 12 }}>
        Nexus Sync is the add-on sync service with end-to-end encryption and version
        history.
      </p>
      <p style={{ marginBottom: 16 }}>
        To start syncing, please log in or create a new Nexus account.
      </p>
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button
          type="button"
          onClick={() =>
            window.open('https://github.com/baileyrd/nexus#sync', '_blank')
          }
          style={{
            background: 'var(--interactive-accent)',
            color: 'var(--interactive-accent-ink)',
            border: 'none',
            borderRadius: 4,
            padding: '6px 14px',
            fontSize: 13,
            cursor: 'pointer',
          }}
        >
          Sign up
        </button>
        <button
          type="button"
          onClick={() =>
            window.open('https://github.com/baileyrd/nexus#sync', '_blank')
          }
          style={{
            background: 'var(--background-modifier-hover)',
            color: 'var(--text-normal)',
            border: 'none',
            borderRadius: 4,
            padding: '6px 14px',
            fontSize: 13,
            cursor: 'pointer',
          }}
        >
          Log in
        </button>
      </div>
    </div>
  )
}

function StubTemplatesPage(_: { api?: PluginAPI }) {
  const now = new Date()
  const today = now.toISOString().slice(0, 10)
  const time = now.toTimeString().slice(0, 5)
  return (
    <div className="settings-section">
      <StubRow
        title="Template folder location"
        description="Files in this folder will be available as templates."
        control={
          <WiredText
            settingKey="nexus.settings.templates.folderLocation"
            defaultValue=""
            placeholder="Example: folder 1/folder 2"
            label="Template folder location"
          />
        }
      />
      <StubRow
        title="Date format"
        description={
          '{{date}} in the template file will be replaced with this value. ' +
          `Your current syntax looks like this: ${today}`
        }
        control={
          <WiredText
            settingKey="nexus.settings.templates.dateFormat"
            defaultValue=""
            placeholder="YYYY-MM-DD"
            label="Templates date format"
          />
        }
      />
      <StubRow
        title="Time format"
        description={
          '{{time}} in the template file will be replaced with this value. ' +
          `Your current syntax looks like this: ${time}`
        }
        control={
          <WiredText
            settingKey="nexus.settings.templates.timeFormat"
            defaultValue=""
            placeholder="HH:mm"
            label="Templates time format"
          />
        }
      />
    </div>
  )
}

// ─── Plugin-contributed tab body ──────────────────────────────────────────────
//
// OI-01 + OI-08: when the user selects a tab whose id was registered
// via `api.settings.registerTab`, we look up the renderer in the
// SettingsTabRegistry and render it inside the standard panel
// chrome. Falls back to the "Unknown tab" empty state if the renderer
// hasn't been wired (manifest-declared but plugin not yet activated).

function ContributedTabBody({ navTab }: { navTab: NavTab }) {
  const reg = getRegistry()
  const Renderer = reg?.settingsTabs.getRenderer(navTab as string)
  return (
    <div className="settings-body">
      <div className="settings-content">
        {Renderer ? (
          createElement(Renderer, {})
        ) : (
          <p className="settings-empty">
            Unknown tab. Pick one from the left rail.
          </p>
        )}
      </div>
    </div>
  )
}

// ─── Rail item ────────────────────────────────────────────────────────────────

function RailItem({
  label,
  active,
  onClick,
  title,
}: {
  label: string
  active: boolean
  onClick: () => void
  title?: string
}) {
  return (
    <button
      className={`settings-rail-item ${active ? 'settings-rail-item--active' : ''}`}
      onClick={onClick}
      title={title}
    >
      {label}
    </button>
  )
}

// ─── Appearance tab ───────────────────────────────────────────────────────────
//
// WI-02 part 3 — Settings > Appearance UI. Three sections, all routed
// through `useThemeStore` actions which talk to the kernel
// `com.nexus.theme` plugin:
//
//   1. Theme picker (dropdown) — `setActiveTheme`
//   2. Mode radio (light/dark/system) — `setMode`
//   3. Snippets list (checkbox + up/down reorder) —
//      `toggleSnippet` / `setSnippetOrder`
//
// Live preview "just works": the store applies CSS variables to :root
// on every kernel echo (themeStore.applyResolvedVariables), so picking
// a theme repaints the chrome without any extra wiring here.
//
// Reorder UX is up/down buttons rather than HTML5 drag-drop. Drag-drop
// is the nicer affordance but adds enough complexity (focus
// management, accessibility, keyboard fallback) that buttons are the
// right starting point for Phase 2; a follow-up can graduate to drag.

function AppearanceTab({ api }: { api?: PluginAPI }) {
  // Subscribe to the slices we render — Zustand re-renders only when
  // these specific values change, so a snippet toggle won't re-render
  // the theme dropdown and vice-versa.
  const availableThemes   = useThemeStore(s => s.availableThemes)
  const availableSnippets = useThemeStore(s => s.availableSnippets)
  const activeThemeId     = useThemeStore(s => s.activeThemeId)
  const enabledSnippets   = useThemeStore(s => s.enabledSnippets)
  const loaded            = useThemeStore(s => s.loaded)

  const [busy,  setBusy]  = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Centralised wrapper around the store actions. Sets the busy flag
  // (drives the dropdown's `disabled` attr so the user can't fire two
  // theme switches in flight) and surfaces kernel errors as a banner
  // rather than crashing the panel.
  const run = useCallback(
    async (label: string, fn: () => Promise<void>) => {
      if (!api) {
        setError(`${label}: settings panel is not wired to the kernel API yet`)
        return
      }
      setBusy(true)
      setError(null)
      try {
        await fn()
      } catch (err) {
        const reason = err instanceof Error ? err.message : String(err)
        setError(`${label} failed: ${reason}`)
      } finally {
        setBusy(false)
      }
    },
    [api],
  )

  const handleThemeChange = (id: string) => {
    void run('Apply theme', () =>
      useThemeStore.getState().setActiveTheme(api!, id),
    )
  }

  const handleSnippetToggle = (id: string) => {
    void run('Toggle snippet', () =>
      useThemeStore.getState().toggleSnippet(api!, id),
    )
  }

  // Up/down reorder: build a fresh enabled-id list from the current
  // store ordering, swap adjacent ids, and ship the whole list to the
  // kernel via setSnippetOrder. Disabled snippets aren't part of the
  // ordered list (the kernel only stores enabled ids in cascade order).
  const handleReorder = (id: string, direction: 'up' | 'down') => {
    const idx = enabledSnippets.indexOf(id)
    if (idx === -1) return
    const swap = direction === 'up' ? idx - 1 : idx + 1
    if (swap < 0 || swap >= enabledSnippets.length) return
    const next = [...enabledSnippets]
    ;[next[idx], next[swap]] = [next[swap], next[idx]]
    void run('Reorder snippets', () =>
      useThemeStore.getState().setSnippetOrder(api!, next),
    )
  }

  // Render snippets in two groups: enabled (in cascade order, with
  // up/down controls) followed by disabled (alphabetical, just a
  // checkbox). Mirrors the legacy shell's settings layout intent —
  // a cascading order needs visible hierarchy.
  const enabledList = useMemo(
    () =>
      enabledSnippets
        .map(id => availableSnippets.find(s => s.id === id))
        .filter((s): s is AvailableSnippet => Boolean(s)),
    [enabledSnippets, availableSnippets],
  )
  const disabledList = useMemo(
    () =>
      availableSnippets
        .filter(s => !enabledSnippets.includes(s.id))
        .slice()
        .sort((a, b) => a.name.localeCompare(b.name)),
    [availableSnippets, enabledSnippets],
  )

  // Derived: render hint for the theme dropdown.
  const activeMeta = availableThemes.find((t) => t.id === activeThemeId)
  const activeCategory =
    typeof activeMeta?.category === 'string' ? activeMeta.category : undefined
  const scheme: 'light' | 'dark' =
    activeCategory === 'light' ? 'light' : 'dark'


  return (
    <div className="settings-section">
      {error && (
        <div
          role="alert"
          style={{
            padding: 8,
            marginBottom: 12,
            background: 'var(--risk-soft)',
            color: 'var(--risk)',
            borderRadius: 4,
          }}
        >
          {error}
        </div>
      )}

      {/* ── Top group ─────────────────────────────────────────── */}
      <StubRow
        title="Accent color"
        description="Choose the accent color used throughout the app."
        control={
          <WiredAccentColor settingKey="nexus.settings.appearance.accentColor" />
        }
      />
      <StubRow
        title="Themes"
        description="Manage installed themes and browse community themes."
        control={
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <select
              value={activeThemeId ?? ''}
              disabled={busy || !loaded || availableThemes.length === 0}
              onChange={(e) => handleThemeChange(e.target.value)}
              style={{
                minWidth: 160,
                colorScheme: scheme,
              }}
              title="Active theme"
            >
              {availableThemes.length === 0 && (
                <option value="">{loaded ? 'No themes installed' : 'Loading...'}</option>
              )}
              {availableThemes.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))}
            </select>
            <button
              type="button"
              onClick={() =>
                window.open(
                  'https://github.com/baileyrd/nexus#community-themes',
                  '_blank',
                )
              }
              style={{
                background: 'var(--interactive-accent)',
                color: 'var(--interactive-accent-ink)',
                border: 'none',
                borderRadius: 4,
                padding: '4px 12px',
                fontSize: 13,
                cursor: 'pointer',
              }}
            >
              Manage
            </button>
          </div>
        }
      />
      <StubRow
        title="Current community themes"
        description="You currently have 0 themes installed."
        control={<span style={{ color: 'var(--text-muted)', fontSize: 12 }}>—</span>}
      />

      {/* ── Interface (stubs) ─────────────────────────────────── */}
      <div className="settings-section-title" style={{ marginTop: 24 }}>Interface</div>
      <StubRow
        title="Inline title"
        description="Display the filename as an editable title inline with the file contents."
        control={
          <WiredToggle
            settingKey="nexus.settings.appearance.inlineTitle"
            defaultValue={true}
            label="Toggle inline title"
          />
        }
      />
      <StubRow
        title="Show tab title bar"
        description="Display the header at the top of every tab."
        control={
          <WiredToggle
            settingKey="nexus.settings.appearance.showTabTitleBar"
            defaultValue={true}
            label="Toggle tab title bar"
          />
        }
      />
      <StubRow
        title="Show ribbon"
        description="Display vertical toolbar on the side of the window."
        control={
          <WiredToggle
            settingKey="nexus.settings.appearance.showRibbon"
            defaultValue={true}
            label="Toggle ribbon"
          />
        }
      />
      <StubRow
        title="Ribbon menu configuration"
        description="Comma-separated command ids the ribbon should expose. Honored once a ribbon renderer ships."
        control={
          <WiredText
            settingKey="nexus.settings.appearance.ribbonCommands"
            defaultValue=""
            placeholder="nexus.commandPalette.toggle, nexus.editor.toggleMode"
            label="Ribbon commands"
          />
        }
      />

      {/* ── Font (stubs) ──────────────────────────────────────── */}
      <div className="settings-section-title" style={{ marginTop: 24 }}>Font</div>
      <StubRow
        title="Interface font"
        description="Set base font for all of Nexus. Comma-separated CSS font-family stack."
        control={
          <WiredText
            settingKey="nexus.settings.appearance.fontInterface"
            defaultValue=""
            placeholder="system-ui, -apple-system, sans-serif"
            label="Interface font"
          />
        }
      />
      <StubRow
        title="Text font"
        description="Set font for editing and reading views. Comma-separated CSS font-family stack."
        control={
          <WiredText
            settingKey="nexus.settings.appearance.fontText"
            defaultValue=""
            placeholder="ui-serif, Georgia, serif"
            label="Text font"
          />
        }
      />
      <StubRow
        title="Monospace font"
        description="Set font for places like code blocks and frontmatter. Comma-separated CSS font-family stack."
        control={
          <WiredText
            settingKey="nexus.settings.appearance.fontMonospace"
            defaultValue=""
            placeholder="ui-monospace, SFMono-Regular, Menlo, monospace"
            label="Monospace font"
          />
        }
      />
      <StubRow
        title="Font size"
        description="Font size in pixels that affects editing and reading views."
        control={
          <WiredNumberRange
            settingKey="nexus.settings.appearance.fontSize"
            defaultValue={14}
            min={10}
            max={24}
            label="Font size"
          />
        }
      />
      <StubRow
        title="Quick font size adjustment"
        description="Adjust the font size using Ctrl + Scroll, or using the trackpad pinch-zoom gesture."
        control={
          <WiredToggle
            settingKey="nexus.settings.appearance.quickFontAdjust"
            defaultValue={false}
            label="Toggle quick font size adjustment"
          />
        }
      />

      {/* ── Advanced (stubs) ──────────────────────────────────── */}
      <div className="settings-section-title" style={{ marginTop: 24 }}>Advanced</div>
      <StubRow
        title="Zoom level"
        description="Controls the overall zoom level of the app."
        control={
          <WiredNumberRange
            settingKey="nexus.settings.appearance.zoomLevel"
            defaultValue={100}
            min={50}
            max={200}
            label="Zoom level"
          />
        }
      />
      <StubRow
        title="Native menus"
        description="Menus throughout the app will match the operating system. They will not be affected by your theme."
        control={
          <WiredToggle
            settingKey="nexus.settings.appearance.nativeMenus"
            defaultValue={false}
            label="Toggle native menus"
          />
        }
      />
      <StubRow
        title="Window frame style"
        description="Determines the styling of the title bar of Nexus windows. Requires a full restart to take effect."
        control={
          <WiredSelect
            settingKey="nexus.settings.appearance.windowFrame"
            defaultValue="hidden"
            label="Window frame style"
            options={[
              { value: 'hidden', label: 'Hidden (default)' },
              { value: 'native', label: 'Native' },
              { value: 'custom', label: 'Custom' },
            ]}
          />
        }
      />
      <StubRow
        title="Custom app icon"
        description="Set a custom icon for the app. Path is saved in app.toml; a future packaging step will pick it up."
        control={
          <CustomAppIconChooser api={api} />
        }
      />
      <StubRow
        title="Hardware acceleration"
        description={
          'Turns on hardware acceleration, which uses your GPU to make Nexus smoother. ' +
          'If you turn this off, app performance will be severely degraded.'
        }
        control={
          <WiredToggle
            settingKey="nexus.settings.appearance.hardwareAcceleration"
            defaultValue={true}
            label="Toggle hardware acceleration"
          />
        }
      />

      {/* ── CSS snippets (real controls) ─────────────────────── */}
      <div className="settings-section-title" style={{ marginTop: 24 }}>CSS snippets</div>
      <p className="settings-field-description">
        Layered after the theme. Order matters — later snippets override earlier ones.
      </p>
      {availableSnippets.length === 0 ? (
        <p className="settings-empty" style={{ marginTop: 12 }}>
          No CSS snippets found. Drop a <code>.css</code> file into your snippets directory and restart.
        </p>
      ) : (
        <>
          {enabledList.length > 0 && (
            <div style={{ marginTop: 12 }}>
              <div style={{ fontSize: '0.85em', opacity: 0.6, marginBottom: 4 }}>
                Enabled (cascade order, top → bottom)
              </div>
              <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
                {enabledList.map((s, i) => (
                  <SnippetRow
                    key={s.id}
                    snippet={s}
                    enabled
                    busy={busy}
                    canMoveUp={i > 0}
                    canMoveDown={i < enabledList.length - 1}
                    onToggle={() => handleSnippetToggle(s.id)}
                    onMoveUp={() => handleReorder(s.id, 'up')}
                    onMoveDown={() => handleReorder(s.id, 'down')}
                  />
                ))}
              </ul>
            </div>
          )}
          {disabledList.length > 0 && (
            <div style={{ marginTop: 16 }}>
              <div style={{ fontSize: '0.85em', opacity: 0.6, marginBottom: 4 }}>Available</div>
              <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
                {disabledList.map((s) => (
                  <SnippetRow
                    key={s.id}
                    snippet={s}
                    enabled={false}
                    busy={busy}
                    canMoveUp={false}
                    canMoveDown={false}
                    onToggle={() => handleSnippetToggle(s.id)}
                    onMoveUp={() => {}}
                    onMoveDown={() => {}}
                  />
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </div>
  )
}

function SnippetRow({
  snippet, enabled, busy, canMoveUp, canMoveDown, onToggle, onMoveUp, onMoveDown,
}: {
  snippet:     AvailableSnippet
  enabled:     boolean
  busy:        boolean
  canMoveUp:   boolean
  canMoveDown: boolean
  onToggle:    () => void
  onMoveUp:    () => void
  onMoveDown:  () => void
}) {
  return (
    <li
      style={{
        display: 'flex',
        alignItems: 'flex-start',
        gap: 8,
        padding: '8px 6px',
        borderBottom: '1px solid var(--background-modifier-border)',
      }}
    >
      <input
        type="checkbox"
        checked={enabled}
        disabled={busy}
        onChange={onToggle}
        aria-label={`Enable ${snippet.name}`}
        style={{ marginTop: 3 }}
      />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontWeight: 500 }}>{snippet.name}</div>
        {snippet.description && (
          <div style={{ fontSize: '0.85em', opacity: 0.7 }}>{snippet.description}</div>
        )}
        <div style={{ fontSize: '0.75em', opacity: 0.5 }}>{snippet.id}</div>
      </div>
      {enabled && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          <button
            type="button"
            onClick={onMoveUp}
            disabled={busy || !canMoveUp}
            aria-label={`Move ${snippet.name} up`}
            title="Move up"
            style={{ padding: '2px 6px', fontSize: '0.75em' }}
          >
            ▲
          </button>
          <button
            type="button"
            onClick={onMoveDown}
            disabled={busy || !canMoveDown}
            aria-label={`Move ${snippet.name} down`}
            title="Move down"
            style={{ padding: '2px 6px', fontSize: '0.75em' }}
          >
            ▼
          </button>
        </div>
      )}
    </li>
  )
}

// ─── Keybindings tab ──────────────────────────────────────────────────────────

interface BindingDisplayRow extends BindingRow {
  title: string
  category?: string
}

function useBindingRows(refreshNonce: number): BindingDisplayRow[] {
  return useMemo(() => {
    void refreshNonce // re-derive when nonce bumps
    const reg = getRegistry()
    if (!reg) return []
    const cmdById = new Map(reg.commands.all().map(c => [c.id, c]))
    return reg.keybindings.getAllBindings().map(row => {
      const cmd = cmdById.get(row.commandId)
      return {
        ...row,
        title: cmd?.title ?? row.commandId,
        category: cmd?.category,
      }
    })
  }, [refreshNonce])
}

function KeybindingsTab() {
  const seededQuery = useContextKey('settingsHotkeysQuery') as string | undefined
  const [query, setQuery] = useState(seededQuery ?? '')
  const [editing, setEditing] = useState<string | null>(null)
  const [nonce, setNonce] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const rows = useBindingRows(nonce)

  // One-shot consume: if a sibling component (e.g. Core plugins page's
  // per-row + button) seeded a query before navigating us here, apply
  // it once then clear so subsequent visits start fresh.
  useEffect(() => {
    if (typeof seededQuery === 'string' && seededQuery !== '') {
      setQuery(seededQuery)
      useContextKeyStore.getState().set('settingsHotkeysQuery', undefined)
    }
  }, [seededQuery])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return rows
    return rows.filter(r =>
      r.title.toLowerCase().includes(q) ||
      r.commandId.toLowerCase().includes(q) ||
      r.current.toLowerCase().includes(q),
    )
  }, [rows, query])

  const conflictCount = useMemo(
    () => rows.reduce((n, r) => n + (r.conflictsWith.length > 0 ? 1 : 0), 0),
    [rows],
  )

  const handleCommit = useCallback(async (commandId: string, chord: string) => {
    setError(null)
    const reg = getRegistry()
    if (!reg) return
    try {
      await reg.keybindings.setOverride(commandId, chord)
      setEditing(null)
      setNonce(n => n + 1)
    } catch (err) {
      setError(`Failed to save override: ${err instanceof Error ? err.message : String(err)}`)
    }
  }, [])

  const handleReset = useCallback(async (commandId: string) => {
    setError(null)
    const reg = getRegistry()
    if (!reg) return
    try {
      await reg.keybindings.clearOverride(commandId)
      setNonce(n => n + 1)
    } catch (err) {
      setError(`Failed to reset override: ${err instanceof Error ? err.message : String(err)}`)
    }
  }, [])

  return (
    <div className="settings-section">
      <div
        style={{
          display: 'flex',
          alignItems: 'flex-start',
          gap: 12,
          marginBottom: 16,
        }}
      >
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className="settings-field-title">Search hotkeys</div>
          <div className="settings-field-description" style={{ marginBottom: 0 }}>
            Showing {rows.length} hotkey{rows.length === 1 ? '' : 's'}.
          </div>
        </div>
        <span
          aria-hidden="true"
          title="Filter (coming soon)"
          style={{
            color: 'var(--text-muted)',
            fontSize: 14,
            paddingTop: 4,
            cursor: 'default',
          }}
        >
          ▽
        </span>
        <input
          type="search"
          className="settings-search"
          placeholder="Filter..."
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          style={{ minWidth: 180, maxWidth: 260, marginTop: 2 }}
        />
      </div>

      {error && (
        <div
          role="alert"
          style={{
            padding: 8,
            marginBottom: 12,
            background: 'var(--risk-soft)',
            color: 'var(--risk)',
            borderRadius: 4,
          }}
        >
          {error}
        </div>
      )}

      {conflictCount > 0 && (
        <div
          role="status"
          style={{
            padding: 8,
            marginBottom: 12,
            background: 'var(--color-warning-bg)',
            color: 'var(--color-warning)',
            borderRadius: 4,
            fontSize: '0.9em',
          }}
        >
          {conflictCount === 1
            ? '1 command is bound to a chord that another command also claims.'
            : `${conflictCount} commands are bound to chords that other commands also claim.`}
          {' Override one of them to resolve the conflict.'}
        </div>
      )}

      {filtered.length === 0 ? (
        <p className="settings-empty">No keybindings match.</p>
      ) : (
        <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
          {filtered.map((row) => (
            <li
              key={row.commandId}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 12,
                padding: '12px 4px',
                borderBottom: '1px solid var(--background-modifier-border)',
              }}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 13 }}>
                  <span style={{ color: 'var(--text-normal)' }}>{row.title}</span>
                  {row.overridden && (
                    <span
                      title="Override active"
                      style={{
                        display: 'inline-block',
                        width: 6,
                        height: 6,
                        borderRadius: '50%',
                        background: 'var(--interactive-accent)',
                      }}
                    />
                  )}
                  {row.conflictsWith.length > 0 && (
                    <span
                      title={`Chord conflict — also bound to: ${row.conflictsWith.join(', ')}`}
                      aria-label="Keybinding conflict"
                      style={{
                        padding: '0 5px',
                        fontSize: '0.7em',
                        fontWeight: 600,
                        lineHeight: '14px',
                        color: 'var(--color-warning)',
                        background: 'var(--color-warning-bg)',
                        border: '1px solid var(--color-warning)',
                        borderRadius: 3,
                      }}
                    >
                      !
                    </span>
                  )}
                </div>
              </div>

              {editing === row.commandId ? (
                <ChordCaptureInput
                  onCommit={(chord) => void handleCommit(row.commandId, chord)}
                  onCancel={() => setEditing(null)}
                />
              ) : row.current ? (
                <span
                  style={{
                    display: 'inline-flex',
                    alignItems: 'center',
                    gap: 4,
                    background: row.overridden
                      ? 'var(--interactive-accent-soft)'
                      : 'var(--background-modifier-hover)',
                    color: 'var(--text-normal)',
                    padding: '2px 8px',
                    borderRadius: 4,
                    fontSize: 12,
                    fontFamily: 'var(--font-monospace, monospace)',
                  }}
                >
                  {formatChord(row.current)}
                  {row.overridden && (
                    <button
                      type="button"
                      onClick={() => void handleReset(row.commandId)}
                      title={`Reset to default (${formatChord(row.default) || '—'})`}
                      aria-label="Reset to default"
                      style={{
                        background: 'transparent',
                        border: 'none',
                        color: 'var(--text-muted)',
                        cursor: 'pointer',
                        padding: 0,
                        fontSize: 12,
                        lineHeight: 1,
                      }}
                    >
                      ✕
                    </button>
                  )}
                </span>
              ) : (
                <span
                  style={{
                    background: 'var(--background-modifier-hover)',
                    color: 'var(--text-muted)',
                    padding: '2px 8px',
                    borderRadius: 4,
                    fontSize: 12,
                  }}
                >
                  Blank
                </span>
              )}

              {editing !== row.commandId && (
                <button
                  type="button"
                  onClick={() => setEditing(row.commandId)}
                  title="Add or change shortcut"
                  aria-label="Edit shortcut"
                  style={{
                    width: 22,
                    height: 22,
                    borderRadius: '50%',
                    border: '1px solid var(--background-modifier-border)',
                    background: 'transparent',
                    color: 'var(--text-muted)',
                    cursor: 'pointer',
                    display: 'inline-grid',
                    placeItems: 'center',
                    fontSize: 14,
                    lineHeight: 1,
                    padding: 0,
                  }}
                >
                  +
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

/**
 * Chord-capture input. Listens for keydown, builds a normalised chord
 * from modifier flags + the first non-modifier key, then auto-commits.
 *
 *   - Esc cancels (without committing).
 *   - Modifier-only presses (Shift, Ctrl, Alt, Meta) are ignored — we
 *     wait for a real key to land before treating the chord as
 *     complete. This means `Shift+P` commits when `P` is hit while
 *     Shift is held, not when Shift alone is held.
 *   - The displayed value uses `formatChord` (Title-Case parts joined
 *     by `+`); the committed value is `normalizeChord`'d (lowercase,
 *     canonical modifier order).
 */
function ChordCaptureInput({
  onCommit,
  onCancel,
}: {
  onCommit: (chord: string) => void
  onCancel: () => void
}) {
  const [pending, setPending] = useState('')
  const ref = useRef<HTMLInputElement>(null)

  useEffect(() => {
    ref.current?.focus()
  }, [])

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.preventDefault()
    e.stopPropagation()

    if (e.key === 'Escape') {
      onCancel()
      return
    }

    const key = e.key.toLowerCase()
    const isModifier = ['control', 'shift', 'alt', 'meta'].includes(key)

    // Build the in-progress display so the user can see the modifiers
    // they're currently holding before they commit by pressing a key.
    const parts: string[] = []
    if (e.ctrlKey)  parts.push('ctrl')
    if (e.shiftKey) parts.push('shift')
    if (e.altKey)   parts.push('alt')
    if (e.metaKey)  parts.push('meta')

    if (isModifier) {
      // Show the modifier(s) being held, but don't commit yet.
      setPending(parts.length ? formatChord(parts.join('+')) + '+...' : '')
      return
    }

    parts.push(key)
    const chord = normalizeChord(parts.join('+'))
    setPending(formatChord(chord))
    onCommit(chord)
  }

  return (
    <input
      ref={ref}
      type="text"
      readOnly
      value={pending}
      placeholder="Press a chord..."
      onKeyDown={handleKeyDown}
      onBlur={onCancel}
      style={{
        width: '100%',
        padding: '4px 6px',
        border: '1px solid var(--interactive-accent)',
        borderRadius: 3,
        background: 'var(--background-primary)',
      }}
    />
  )
}


// ─── Settings tab components ──────────────────────────────────────────────────

function SettingsSection({ section }: { section: ConfigSection }) {
  return (
    <div className="settings-section">
      <h2 className="settings-section-title">{section.title}</h2>
      {section.schema.map(field => (
        <SettingsField key={field.key} field={field} />
      ))}
    </div>
  )
}

function SettingsField({ field }: { field: ConfigSchema }) {
  const value    = useConfigValue(field.key, field.default)
  const setValue = useConfigStore(s => s.set)

  const renderControl = () => {
    switch (field.type) {
      case 'boolean':
        return (
          <input
            id={field.key}
            type="checkbox"
            checked={value as boolean}
            onChange={e => setValue(field.key, e.target.checked)}
          />
        )
      case 'select':
        return (
          <select
            id={field.key}
            value={value as string}
            onChange={e => setValue(field.key, e.target.value)}
          >
            {field.options?.map(o => (
              <option key={o} value={o}>{o}</option>
            ))}
          </select>
        )
      case 'number':
        return (
          <input
            id={field.key}
            type="number"
            value={value as number}
            onChange={e => setValue(field.key, Number(e.target.value))}
          />
        )
      case 'password':
        // Same shape as 'string' but masked. Used for API keys —
        // browser never auto-fills these (autoComplete=new-password)
        // and `spellCheck` off avoids dictionary squiggles on the
        // gibberish.
        return (
          <input
            id={field.key}
            type="password"
            value={(value as string) ?? ''}
            autoComplete="new-password"
            spellCheck={false}
            placeholder={field.default ? String(field.default) : '••••••••'}
            onChange={e => setValue(field.key, e.target.value)}
            style={{ minWidth: 280 }}
          />
        )
      case 'string':
      default:
        return (
          <input
            id={field.key}
            type="text"
            value={value as string}
            onChange={e => setValue(field.key, e.target.value)}
          />
        )
    }
  }

  return (
    <div className="settings-field">
      <div className="settings-field-header">
        <label htmlFor={field.key} className="settings-field-title">
          {field.title}
        </label>
        {field.type === 'boolean' && renderControl()}
      </div>
      <p className="settings-field-description">{field.description}</p>
      {field.type !== 'boolean' && (
        <div className="settings-field-control">{renderControl()}</div>
      )}
    </div>
  )
}

// ─── Snippets tab (OI-18) ─────────────────────────────────────────────────────

function useSnippetRows(): SnippetEntry[] {
  return useMemo(() => {
    const reg = getRegistry()
    if (!reg) return []
    return reg.snippets.all()
  }, [])
}

function useSnippetConflicts(): SnippetConflict[] {
  const [conflicts, setConflicts] = useState<SnippetConflict[]>(() => {
    const reg = getRegistry()
    return reg?.snippets.getConflicts() ?? []
  })
  useEffect(() => {
    return eventBus.on<{ conflicts: SnippetConflict[] }>('plugins:snippets-conflict', (payload) => {
      setConflicts(payload.conflicts)
    })
  }, [])
  return conflicts
}

function SnippetsTab() {
  const [query, setQuery] = useState('')
  const rows = useSnippetRows()
  const conflicts = useSnippetConflicts()

  const conflictTriggers = useMemo(
    () => new Set(conflicts.map(c => c.trigger)),
    [conflicts],
  )

  const filtered = useMemo(
    () =>
      query
        ? rows.filter(
            r =>
              r.trigger.toLowerCase().includes(query.toLowerCase()) ||
              r.id.toLowerCase().includes(query.toLowerCase()) ||
              r.pluginId.toLowerCase().includes(query.toLowerCase()),
          )
        : rows,
    [rows, query],
  )

  const conflictCount = conflicts.length

  return (
    <div className="keybindings-tab">
      <h3 style={{ marginTop: 0 }}>Snippets</h3>
      <p className="settings-help" style={{ marginBottom: '1rem' }}>
        Text-expansion snippets registered by plugins. Type a trigger string in
        the editor and press Tab to expand it. Conflicts occur when two plugins
        claim the same trigger — last-registered wins.
      </p>

      <input
        className="settings-search"
        placeholder="Filter by trigger, id, or plugin…"
        value={query}
        onChange={e => setQuery(e.target.value)}
        style={{ marginBottom: 12 }}
      />

      {conflictCount > 0 && (
        <div
          role="status"
          style={{
            padding: 8,
            marginBottom: 12,
            background: 'var(--color-warning-bg)',
            color: 'var(--color-warning)',
            borderRadius: 4,
            fontSize: '0.9em',
          }}
        >
          {conflictCount === 1
            ? '1 trigger is claimed by more than one plugin.'
            : `${conflictCount} triggers are claimed by more than one plugin.`}
          {' The last-registered snippet wins.'}
        </div>
      )}

      {filtered.length === 0 ? (
        <p className="settings-empty">
          {query ? 'No snippets match.' : 'No snippets registered.'}
        </p>
      ) : (
        <table style={{ width: '100%', borderCollapse: 'collapse' }}>
          <thead>
            <tr style={{ textAlign: 'left', borderBottom: '1px solid var(--background-modifier-border)' }}>
              <th style={{ padding: '0.4rem 0.5rem' }}>Trigger</th>
              <th style={{ padding: '0.4rem 0.5rem' }}>Body</th>
              <th style={{ padding: '0.4rem 0.5rem' }}>Plugin</th>
              <th style={{ padding: '0.4rem 0.5rem' }}>File types</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map(row => {
              const isConflict = conflictTriggers.has(row.trigger)
              return (
                <tr
                  key={row.id}
                  style={{ borderBottom: '1px solid var(--background-modifier-border)' }}
                >
                  <td style={{ padding: '0.4rem 0.5rem', fontFamily: 'var(--font-monospace)', whiteSpace: 'nowrap' }}>
                    {row.trigger}
                    {isConflict && (
                      <span
                        title="Trigger conflict — more than one plugin registered this trigger"
                        aria-label="Snippet trigger conflict"
                        style={{
                          display: 'inline-block',
                          marginLeft: 6,
                          padding: '0 5px',
                          fontSize: '0.7em',
                          fontWeight: 600,
                          lineHeight: '14px',
                          color: 'var(--color-warning)',
                          background: 'var(--color-warning-bg)',
                          border: '1px solid var(--color-warning)',
                          borderRadius: 3,
                          verticalAlign: 'middle',
                        }}
                      >
                        {'!'}
                      </span>
                    )}
                  </td>
                  <td style={{ padding: '0.4rem 0.5rem', fontFamily: 'var(--font-monospace)', color: 'var(--text-muted)', maxWidth: 300, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {row.body}
                  </td>
                  <td style={{ padding: '0.4rem 0.5rem', color: 'var(--text-muted)', fontSize: '0.9em' }}>
                    {row.pluginId}
                  </td>
                  <td style={{ padding: '0.4rem 0.5rem', color: 'var(--text-faint)', fontSize: '0.85em' }}>
                    {row.fileTypes?.join(', ') ?? '—'}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      )}
    </div>
  )
}
