// src/plugins/core/settings/SettingsPanelView.tsx
// Auto-generates settings UI from registered config schemas.
// Plugins tab: lists core plugins + discovered community plugins with toggles.

import { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { getRegistry } from '../../../host/shellRegistry'
import { useContextKey, useContextKeyStore } from '../../../host/ContextKeyService'
import { useConfigStore, useConfigValue } from '../../../stores/configStore'
import {
  useThemeStore,
  type AvailableSnippet,
  type ThemeMode,
} from '../../../stores/themeStore'
import type { ConfigSection, ConfigSchema, PluginAPI } from '../../../types/plugin'
import type { CommunityPluginManifest } from '../../../host/communityPluginLoader'
import {
  formatChord,
  normalizeChord,
  type BindingRow,
  type OverrideStorage,
} from '../../../registry/KeybindingRegistry'

// ─── Types ────────────────────────────────────────────────────────────────────

interface PluginInfo {
  id:      string
  name:    string
  version: string
  core:    boolean
  state:   string
  error?:  string
}

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

// Re-read services once boot completes (shellReady flips to true after
// pluginList and communityPluginManifests are both registered).
function usePluginList(): PluginInfo[] {
  const [list, setList] = useState<PluginInfo[]>([])
  const shellReady = useContextKey('shellReady')

  useEffect(() => {
    const reg = getRegistry()
    if (!reg) return
    try {
      setList(reg.getService<PluginInfo[]>('pluginList'))
    } catch {
      // not registered yet
    }
  }, [shellReady])

  return list
}

function useCommunityManifests(): CommunityPluginManifest[] {
  const [list, setList] = useState<CommunityPluginManifest[]>([])
  const shellReady = useContextKey('shellReady')

  useEffect(() => {
    const reg = getRegistry()
    if (!reg) return
    try {
      setList(reg.getService<CommunityPluginManifest[]>('communityPluginManifests'))
    } catch {
      // no community plugins discovered
    }
  }, [shellReady])

  return list
}

// ─── Main panel ───────────────────────────────────────────────────────────────

type NavTab = 'settings' | 'appearance' | 'keybindings' | 'plugins'

// ─── Override storage (shared with the plugin's activate() hydrator) ─────────
// Lives at the same `plugin:core.settings:keybinding-overrides` localStorage
// key the settings plugin writes through `api.storage`. The settings panel
// can't import @nexus/extension-api (no api in scope), so we re-implement
// the same key/serialisation here. Both sides round-trip identical JSON.

const OVERRIDES_STORAGE_KEY = 'plugin:core.settings:keybinding-overrides'

export const keybindingOverrideStorage: OverrideStorage = {
  async read() {
    try {
      const raw = localStorage.getItem(OVERRIDES_STORAGE_KEY)
      if (!raw) return {}
      const parsed = JSON.parse(raw) as unknown
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        return parsed as Record<string, string>
      }
      return {}
    } catch {
      return {}
    }
  },
  async write(overrides) {
    localStorage.setItem(OVERRIDES_STORAGE_KEY, JSON.stringify(overrides))
  },
}

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
  const plugins    = usePluginList()
  const community  = useCommunityManifests()

  const [navTab,        setNavTab]        = useState<NavTab>('settings')
  const [query,         setQuery]         = useState('')
  const [activeSection, setActiveSection] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  // Honour `settingsActiveTab` context key set by openKeybindings command.
  useEffect(() => {
    if (visible && requestedTab) {
      setNavTab(requestedTab)
      // Clear the request so subsequent opens don't re-route.
      useContextKeyStore.getState().set('settingsActiveTab', undefined)
    }
  }, [visible, requestedTab])

  const close = () => {
    useContextKeyStore.getState().set('settingsPanelVisible', false)
    setQuery('')
  }

  useEffect(() => {
    if (sections.length > 0 && !activeSection) {
      setActiveSection(sections[0].pluginId)
    }
  }, [sections, activeSection])

  useEffect(() => {
    if (visible) setTimeout(() => inputRef.current?.focus(), 0)
  }, [visible])

  if (!visible) return null

  const displayedSections = query
    ? sections
        .map(s => ({
          ...s,
          schema: s.schema.filter(f =>
            f.title.toLowerCase().includes(query.toLowerCase()) ||
            f.description.toLowerCase().includes(query.toLowerCase()) ||
            f.key.toLowerCase().includes(query.toLowerCase())
          ),
        }))
        .filter(s => s.schema.length > 0)
    : sections.filter(s => !activeSection || s.pluginId === activeSection)

  return (
    <div
      className="settings-backdrop"
      onClick={close}
      style={{ pointerEvents: 'auto' }}
    >
      <div
        className="settings-panel"
        onClick={e => e.stopPropagation()}
        onKeyDown={e => e.key === 'Escape' && close()}
      >
        {/* Header */}
        <div className="settings-header">
          <div style={{ display: 'flex', gap: 0, flexShrink: 0 }}>
            <button
              className={`settings-nav-tab ${navTab === 'settings' ? 'settings-nav-tab--active' : ''}`}
              onClick={() => setNavTab('settings')}
            >
              Settings
            </button>
            <button
              className={`settings-nav-tab ${navTab === 'appearance' ? 'settings-nav-tab--active' : ''}`}
              onClick={() => setNavTab('appearance')}
            >
              Appearance
            </button>
            <button
              className={`settings-nav-tab ${navTab === 'keybindings' ? 'settings-nav-tab--active' : ''}`}
              onClick={() => setNavTab('keybindings')}
            >
              Keybindings
            </button>
            <button
              className={`settings-nav-tab ${navTab === 'plugins' ? 'settings-nav-tab--active' : ''}`}
              onClick={() => setNavTab('plugins')}
            >
              Plugins
            </button>
          </div>

          {navTab === 'settings' && (
            <input
              ref={inputRef}
              className="settings-search"
              placeholder="Search settings..."
              value={query}
              onChange={e => setQuery(e.target.value)}
            />
          )}

          <button className="settings-close" onClick={close}>✕</button>
        </div>

        {/* Body */}
        {navTab === 'settings' && (
          <div className="settings-body">
            {!query && (
              <nav className="settings-nav">
                {sections.map(s => (
                  <button
                    key={s.pluginId}
                    className={`settings-nav-item ${activeSection === s.pluginId ? 'settings-nav-item--active' : ''}`}
                    onClick={() => setActiveSection(s.pluginId)}
                  >
                    {s.title}
                  </button>
                ))}
              </nav>
            )}

            <div className="settings-content">
              {displayedSections.length === 0 && (
                <p className="settings-empty">
                  {query ? `No settings found for "${query}"` : 'No settings registered.'}
                </p>
              )}
              {displayedSections.map(section => (
                <SettingsSection key={section.pluginId} section={section} />
              ))}
            </div>
          </div>
        )}
        {navTab === 'appearance' && (
          <div className="settings-body">
            <div className="settings-content" style={{ padding: '16px 24px' }}>
              <AppearanceTab api={api} />
            </div>
          </div>
        )}
        {navTab === 'keybindings' && (
          <div className="settings-body">
            <div className="settings-content" style={{ padding: '16px 24px' }}>
              <KeybindingsTab />
            </div>
          </div>
        )}
        {navTab === 'plugins' && (
          <div className="settings-body">
            <div className="settings-content" style={{ padding: '16px 24px' }}>
              <PluginsTab corePlugins={plugins} community={community} />
            </div>
          </div>
        )}
      </div>
    </div>
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
  const mode              = useThemeStore(s => s.theme)
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

  const handleModeChange = (next: ThemeMode) => {
    void run('Set mode', () => useThemeStore.getState().setMode(api!, next))
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
  // checkbox). This mirrors the legacy `app/src/components/settings`
  // layout intent — a cascading order needs visible hierarchy.
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

  return (
    <div className="appearance-tab">
      <header style={{ marginBottom: 16 }}>
        <h2 style={{ margin: 0 }}>Appearance</h2>
        <p className="settings-section-desc" style={{ margin: '4px 0 0', opacity: 0.75 }}>
          Theme, light/dark mode, and CSS snippet cascade. Changes apply live.
        </p>
      </header>

      {error && (
        <div
          role="alert"
          style={{
            padding: 8,
            marginBottom: 12,
            background: 'var(--color-error-bg, #fdd)',
            color: 'var(--color-error, #900)',
            borderRadius: 4,
          }}
        >
          {error}
        </div>
      )}

      {/* ── Theme picker ── */}
      <section className="settings-section" style={{ marginBottom: 24 }}>
        <h3 className="settings-section-title">Theme</h3>
        <p className="settings-field-description">
          Pick a base palette. Variables apply to :root immediately.
        </p>
        <div className="settings-field-control" style={{ marginTop: 8 }}>
          <select
            value={activeThemeId ?? ''}
            disabled={busy || !loaded || availableThemes.length === 0}
            onChange={e => handleThemeChange(e.target.value)}
            style={{ minWidth: 240 }}
          >
            {availableThemes.length === 0 && (
              <option value="">{loaded ? 'No themes installed' : 'Loading...'}</option>
            )}
            {availableThemes.map(t => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </div>
      </section>

      {/* ── Mode ── */}
      <section className="settings-section" style={{ marginBottom: 24 }}>
        <h3 className="settings-section-title">Mode</h3>
        <p className="settings-field-description">
          Light or dark, or follow the OS preference.
        </p>
        <div role="radiogroup" aria-label="Theme mode" style={{ marginTop: 8, display: 'flex', gap: 16 }}>
          {(['light', 'dark', 'system'] as const).map(m => (
            <label
              key={m}
              style={{ display: 'inline-flex', alignItems: 'center', gap: 6, cursor: busy ? 'wait' : 'pointer' }}
            >
              <input
                type="radio"
                name="theme-mode"
                value={m}
                checked={mode === m}
                disabled={busy}
                onChange={() => handleModeChange(m)}
              />
              <span style={{ textTransform: 'capitalize' }}>{m}</span>
            </label>
          ))}
        </div>
      </section>

      {/* ── Snippets ── */}
      <section className="settings-section">
        <h3 className="settings-section-title">CSS snippets</h3>
        <p className="settings-field-description">
          Layered after the theme. Drag order matters — later snippets
          override earlier ones. Use up/down to reorder.
        </p>

        {availableSnippets.length === 0 ? (
          <p className="settings-empty" style={{ marginTop: 12 }}>
            No snippets installed. Drop a <code>.css</code> file into your
            snippets directory and restart.
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
                <div style={{ fontSize: '0.85em', opacity: 0.6, marginBottom: 4 }}>
                  Available
                </div>
                <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
                  {disabledList.map(s => (
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
      </section>
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
        borderBottom: '1px solid var(--color-border, #e0e0e0)',
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
  const [query,    setQuery]    = useState('')
  const [editing,  setEditing]  = useState<string | null>(null)
  const [nonce,    setNonce]    = useState(0)
  const [error,    setError]    = useState<string | null>(null)
  const rows = useBindingRows(nonce)

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return rows
    return rows.filter(r =>
      r.title.toLowerCase().includes(q) ||
      r.commandId.toLowerCase().includes(q) ||
      r.current.toLowerCase().includes(q),
    )
  }, [rows, query])

  const handleCommit = useCallback(async (commandId: string, chord: string) => {
    setError(null)
    const reg = getRegistry()
    if (!reg) return
    try {
      await reg.keybindings.setOverride(keybindingOverrideStorage, commandId, chord)
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
      await reg.keybindings.clearOverride(keybindingOverrideStorage, commandId)
      setNonce(n => n + 1)
    } catch (err) {
      setError(`Failed to reset override: ${err instanceof Error ? err.message : String(err)}`)
    }
  }, [])

  return (
    <div className="keybindings-tab">
      <header style={{ marginBottom: 12 }}>
        <h2 style={{ margin: 0 }}>Keyboard Shortcuts</h2>
        <p className="settings-section-desc" style={{ margin: '4px 0 0', opacity: 0.75 }}>
          Click a chord to record a new one. Overrides persist across restarts.
        </p>
      </header>

      <input
        type="search"
        className="settings-search"
        placeholder="Filter commands or chords..."
        value={query}
        onChange={e => setQuery(e.target.value)}
        style={{ marginBottom: 12, width: '100%' }}
      />

      {error && (
        <div
          role="alert"
          style={{
            padding: 8,
            marginBottom: 12,
            background: 'var(--color-error-bg, #fdd)',
            color: 'var(--color-error, #900)',
            borderRadius: 4,
          }}
        >
          {error}
        </div>
      )}

      {filtered.length === 0 ? (
        <p className="settings-empty">No keybindings match.</p>
      ) : (
        <table className="keybindings-table" style={{ width: '100%', borderCollapse: 'collapse' }}>
          <thead>
            <tr>
              <th style={cellStyle}>Command</th>
              <th style={cellStyle}>Current</th>
              <th style={cellStyle}>Default</th>
              <th style={{ ...cellStyle, width: 140 }}>Actions</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map(row => (
              <tr key={row.commandId}>
                <td style={cellStyle}>
                  <div style={{ fontWeight: 500 }}>{row.title}</div>
                  <div style={{ fontSize: '0.85em', opacity: 0.6 }}>{row.commandId}</div>
                </td>
                <td style={cellStyle}>
                  {editing === row.commandId ? (
                    <ChordCaptureInput
                      onCommit={chord => void handleCommit(row.commandId, chord)}
                      onCancel={() => setEditing(null)}
                    />
                  ) : (
                    <code style={{
                      background: row.overridden
                        ? 'var(--color-accent-bg, #e7f0ff)'
                        : 'var(--color-bg-alt, #f3f3f3)',
                      padding: '2px 6px',
                      borderRadius: 3,
                      fontSize: '0.9em',
                    }}>
                      {formatChord(row.current) || '—'}
                    </code>
                  )}
                </td>
                <td style={cellStyle}>
                  <code style={{ opacity: 0.6, fontSize: '0.9em' }}>
                    {formatChord(row.default) || '—'}
                  </code>
                </td>
                <td style={cellStyle}>
                  {editing === row.commandId ? null : (
                    <>
                      <button
                        type="button"
                        onClick={() => setEditing(row.commandId)}
                        style={{ marginRight: 6 }}
                      >
                        Edit
                      </button>
                      {row.overridden && (
                        <button
                          type="button"
                          onClick={() => void handleReset(row.commandId)}
                        >
                          Reset
                        </button>
                      )}
                    </>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}

const cellStyle: React.CSSProperties = {
  textAlign: 'left',
  padding: '8px 6px',
  borderBottom: '1px solid var(--color-border, #e0e0e0)',
  verticalAlign: 'top',
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
        border: '1px solid var(--color-accent, #06f)',
        borderRadius: 3,
        background: 'var(--color-bg, #fff)',
      }}
    />
  )
}

// ─── Plugins tab ──────────────────────────────────────────────────────────────

function PluginsTab({
  corePlugins,
  community,
}: {
  corePlugins: PluginInfo[]
  community:   CommunityPluginManifest[]
}) {
  const [pendingChanges, setPendingChanges] = useState<Record<string, boolean>>({})
  const [saving,         setSaving]         = useState<string | null>(null)

  const handleToggle = async (pluginId: string, enabled: boolean) => {
    setSaving(pluginId)
    try {
      await invoke('set_plugin_enabled', { pluginId, enabled })
      setPendingChanges(prev => ({ ...prev, [pluginId]: true }))
    } catch (err) {
      console.error('[PluginsTab] set_plugin_enabled failed:', err)
    } finally {
      setSaving(null)
    }
  }

  const hasPending = Object.keys(pendingChanges).length > 0
  const errorCount = corePlugins.filter(p => p.state === 'error').length

  return (
    <div className="plugins-tab">
      {/* Restart banner */}
      {hasPending && (
        <div className="plugins-tab__restart-banner">
          <span>Restart required for changes to take effect.</span>
        </div>
      )}

      {/* ── Core plugins ── */}
      <div className="plugins-tab__section-header">
        Core plugins
        <span className="plugins-tab__section-count">{corePlugins.length}</span>
        {errorCount > 0 && (
          <span className="plugins-tab__error-badge">{errorCount} error{errorCount > 1 ? 's' : ''}</span>
        )}
      </div>

      <div className="plugins-tab__list">
        {corePlugins.length === 0 ? (
          <p className="settings-empty">No core plugins loaded.</p>
        ) : (
          corePlugins.map(p => (
            <CorePluginRow key={p.id} plugin={p} />
          ))
        )}
      </div>

      {/* ── Community plugins ── */}
      <div className="plugins-tab__section-header" style={{ marginTop: 24 }}>
        Community plugins
        <span className="plugins-tab__section-count">{community.length}</span>
      </div>

      <div className="plugins-tab__list">
        {community.length === 0 ? (
          <div className="plugins-tab__empty-community">
            <p>No community plugins found.</p>
            <p className="plugins-tab__empty-hint">
              Drop a plugin folder into{' '}
              <code>~/.nexus-shell/plugins/</code> then restart.
              Each folder needs a <code>plugin.json</code> and a bundled{' '}
              <code>index.js</code>.
            </p>
          </div>
        ) : (
          community.map(m => (
            <CommunityPluginRow
              key={m.id}
              manifest={m}
              saving={saving === m.id}
              changed={!!pendingChanges[m.id]}
              onToggle={handleToggle}
            />
          ))
        )}
      </div>
    </div>
  )
}

// ─── Core plugin row (read-only — always enabled) ─────────────────────────────

function CorePluginRow({ plugin }: { plugin: PluginInfo }) {
  return (
    <div className={`plugin-row ${plugin.state === 'error' ? 'plugin-row--error' : ''}`}>
      <div className="plugin-row__dot" data-state={plugin.state} />
      <div className="plugin-row__body">
        <div className="plugin-row__header">
          <span className="plugin-row__name">{plugin.name}</span>
          <span className="plugin-row__id">{plugin.id}</span>
          <span className="plugin-row__badge plugin-row__badge--core">core</span>
          <span className="plugin-row__version">v{plugin.version}</span>
          <span className={`plugin-row__state plugin-row__state--${plugin.state}`}>
            {plugin.state}
          </span>
        </div>
        {plugin.state === 'error' && plugin.error && (
          <div className="plugin-row__error">{plugin.error}</div>
        )}
      </div>
    </div>
  )
}

// ─── Community plugin row (toggleable) ───────────────────────────────────────

function CommunityPluginRow({
  manifest, saving, changed, onToggle,
}: {
  manifest: CommunityPluginManifest
  saving:   boolean
  changed:  boolean
  onToggle: (id: string, enabled: boolean) => void
}) {
  // Optimistic local state
  const [enabled, setEnabled] = useState(manifest.enabled)

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const next = e.target.checked
    setEnabled(next)
    onToggle(manifest.id, next)
  }

  return (
    <div className="plugin-row">
      <div className="plugin-row__dot" data-state={enabled ? 'active' : 'inactive'} />
      <div className="plugin-row__body">
        <div className="plugin-row__header">
          <span className="plugin-row__name">{manifest.name}</span>
          <span className="plugin-row__id">{manifest.id}</span>
          {manifest.author && (
            <span className="plugin-row__author">{manifest.author}</span>
          )}
          <span className="plugin-row__version">v{manifest.version}</span>
          {changed && (
            <span className="plugin-row__restart-pill">restart needed</span>
          )}
          <label className="plugin-row__toggle" title={enabled ? 'Disable' : 'Enable'}>
            <input
              type="checkbox"
              checked={enabled}
              disabled={saving}
              onChange={handleChange}
            />
            <span className="plugin-row__toggle-track" />
          </label>
        </div>
        {manifest.description && (
          <div className="plugin-row__description">{manifest.description}</div>
        )}
      </div>
    </div>
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
