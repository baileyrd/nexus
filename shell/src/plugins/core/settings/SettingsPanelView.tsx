// src/plugins/core/settings/SettingsPanelView.tsx
// Auto-generates settings UI from registered config schemas.
// Plugins tab: lists core plugins + discovered community plugins with toggles.

import { useState, useEffect, useRef, useCallback, useMemo, createElement } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { PLUGIN_API_VERSION, type Capability } from '@nexus/extension-api'
import { getRegistry } from '../../../host/shellRegistry'
import { useContextKey, useContextKeyStore } from '../../../host/ContextKeyService'
import { useConfigStore, useConfigValue } from '../../../stores/configStore'
import {
  useThemeStore,
  type AvailableSnippet,
  type ThemeMode,
} from '../../../stores/themeStore'
import type { ConfigSection, ConfigSchema, PluginAPI, SettingsTabEntry } from '../../../types/plugin'
import type { CommunityPluginManifest } from '../../../host/communityPluginLoader'
import {
  enableBuiltinPlugin,
  disableBuiltinPlugin,
  PLUGIN_LIST_CHANGED_EVENT,
} from '../../../host/pluginActivation'
import { eventBus } from '../../../host/EventBus'
import { DEFAULT_OFF_PLUGINS } from '../../catalog'
import {
  formatChord,
  normalizeChord,
  type BindingRow,
} from '../../../registry/KeybindingRegistry'
import {
  CAPABILITY_INFO,
  bucketByRisk,
  chipColours,
  hasHighRisk,
  parseManifestCapabilities,
  type RiskLevel,
} from '../../nexus/pluginsMgmt/capabilityInfo'
import {
  requestModalConsent,
  capsToKernelStrings,
  kernelStringsToCaps,
  type PriorGrant,
} from '../capabilityPrompt'

// ─── Types ────────────────────────────────────────────────────────────────────

interface PluginInfo {
  id:      string
  name:    string
  version: string
  core:    boolean
  state:   string
  error?:  string
  /**
   * Optional declared capability list (WI-18). Core plugins registered
   * via `main.tsx` legitimately leave this absent — they inherit
   * `Capability::ALL` from bootstrap and don't carry a per-plugin
   * manifest-declared capability set. The row code path renders an
   * "(unknown)" chip group when `capabilities` is undefined, which is
   * the correct "not applicable" signal for core rows. Community
   * plugins surface their declared caps via
   * `CommunityPluginManifest.capabilities` on the sibling list.
   */
  capabilities?: unknown
}

/** Dormant default-off built-in plugin (shipped, not loaded this session). */
interface AvailablePluginInfo {
  id:      string
  name:    string
  version: string
  core:    boolean
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
    const read = () => {
      try {
        setList(reg.getService<PluginInfo[]>('pluginList'))
      } catch {
        // not registered yet
      }
    }
    read()
    // Re-read whenever a plugin transitions to active mid-session so a
    // hot-enabled built-in shows up under "Core plugins" without a reload.
    return eventBus.on(PLUGIN_LIST_CHANGED_EVENT, read)
  }, [shellReady])

  return list
}

// Default-off built-ins exposed by main.tsx as the `availablePlugins` service.
// Empty list when every default-off plugin has already been opted-in.
function useAvailablePlugins(): AvailablePluginInfo[] {
  const [list, setList] = useState<AvailablePluginInfo[]>([])
  const shellReady = useContextKey('shellReady')

  useEffect(() => {
    const reg = getRegistry()
    if (!reg) return
    const read = () => {
      try {
        setList(reg.getService<AvailablePluginInfo[]>('availablePlugins'))
      } catch {
        // service not registered (older boot path) — leave empty
      }
    }
    read()
    // Drop the just-enabled row from "Available (disabled)" the moment
    // the host marks the plugin active.
    return eventBus.on(PLUGIN_LIST_CHANGED_EVENT, read)
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

// Built-in tab ids; plugin-contributed tab ids are opaque strings.
const BUILT_IN_TABS = ['settings', 'appearance', 'keybindings', 'plugins'] as const
type BuiltInTab = (typeof BUILT_IN_TABS)[number]
type NavTab = BuiltInTab | string

// Storage key for the last-opened tab. `api.storage.set` namespaces
// writes under `plugin:<id>:...` so this key resolves to
// `plugin:core.settings:last-tab` — same scheme as keybinding overrides.
const LAST_TAB_STORAGE_KEY = 'plugin:core.settings:last-tab'

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
  const available  = useAvailablePlugins()
  const contributedTabs = useContributedSettingsTabs()

  const [navTab,        setNavTab]        = useState<NavTab>('settings')
  const [query,         setQuery]         = useState('')
  const [activeSection, setActiveSection] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)

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
  // group rather than whichever section was last open.
  useEffect(() => {
    return eventBus.on('settings:focusSection', (pluginId: unknown) => {
      if (typeof pluginId !== 'string') return
      setNavTab('settings')
      setActiveSection(pluginId)
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
        {/* Left rail — built-in tabs only. Per-plugin entries are
            managed from the Plugins tab on the right. */}
        <nav className="settings-rail">
          <div className="settings-rail-group-header">Options</div>
          <RailItem
            label="Settings"
            active={navTab === 'settings'}
            onClick={() => setNavTab('settings')}
          />
          <RailItem
            label="Appearance"
            active={navTab === 'appearance'}
            onClick={() => setNavTab('appearance')}
          />
          <RailItem
            label="Keybindings"
            active={navTab === 'keybindings'}
            onClick={() => setNavTab('keybindings')}
          />
          <RailItem
            label="Plugins"
            active={navTab === 'plugins'}
            onClick={() => setNavTab('plugins')}
          />
          {/* Plugin-contributed tabs (OI-01 + OI-08). Filter by group so
              we can give core/community plugin tabs their own rail
              groups in a follow-up; for now show 'options' inline with
              the built-ins and skip non-options groups. */}
          {contributedTabs
            .filter((t) => (t.group ?? 'options') === 'options')
            .map((t) => (
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
            overrides the tab body with cross-plugin search results
            (Phase C). */}
        <div className="settings-main">
          <div className="settings-topbar">
            <input
              ref={inputRef}
              className="settings-search"
              placeholder="Search settings..."
              value={query}
              onChange={e => setQuery(e.target.value)}
            />
            <button className="settings-close" onClick={close}>✕</button>
          </div>

          {query ? (
            <div className="settings-body">
              <div className="settings-content">
                {displayedSections.length === 0 && (
                  <p className="settings-empty">
                    No settings found for &ldquo;{query}&rdquo;
                  </p>
                )}
                {displayedSections.map(section => (
                  <SettingsSection key={section.pluginId} section={section} />
                ))}
              </div>
            </div>
          ) : (
            <>
              {navTab === 'settings' && (
                <div className="settings-body">
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
                  <div className="settings-content">
                    {displayedSections.length === 0 && (
                      <p className="settings-empty">No settings registered.</p>
                    )}
                    {displayedSections.map(section => (
                      <SettingsSection key={section.pluginId} section={section} />
                    ))}
                  </div>
                </div>
              )}
              {navTab === 'appearance' && (
                <div className="settings-body">
                  <div className="settings-content">
                    <AppearanceTab api={api} />
                  </div>
                </div>
              )}
              {navTab === 'keybindings' && (
                <div className="settings-body">
                  <div className="settings-content">
                    <KeybindingsTab />
                  </div>
                </div>
              )}
              {navTab === 'plugins' && (
                <div className="settings-body">
                  <div className="settings-content">
                    <PluginsTab
                      corePlugins={plugins}
                      community={community}
                      available={available}
                    />
                  </div>
                </div>
              )}
              {!BUILT_IN_TABS.includes(navTab as BuiltInTab) && (
                <ContributedTabBody navTab={navTab} />
              )}
            </>
          )}
        </div>
      </div>
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
    // setMode in themeStore handles the kernel call and then auto-
    // applies a theme of the matching category — no extra coupling
    // needed here.
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
          {(() => {
            // Native <option> elements use OS-rendered chrome; CSS on
            // the parent select doesn't reach them. `color-scheme` is
            // the one hint Chromium honors — set it to match the
            // active theme's category so the popup list renders with
            // the right contrast. Falls back to the user's Mode pick
            // when the active theme has no category metadata, and
            // finally to 'dark' so we never render light-on-light.
            const activeMeta = availableThemes.find((t) => t.id === activeThemeId)
            const activeCategory =
              typeof activeMeta?.category === 'string' ? activeMeta.category : undefined
            const scheme: 'light' | 'dark' =
              activeCategory === 'light'
                ? 'light'
                : activeCategory === 'dark'
                ? 'dark'
                : mode === 'light'
                ? 'light'
                : 'dark'
            return (
              <select
                value={activeThemeId ?? ''}
                disabled={busy || !loaded || availableThemes.length === 0}
                onChange={e => handleThemeChange(e.target.value)}
                style={{
                  minWidth: 240,
                  padding: '4px 8px',
                  background: 'var(--background-primary)',
                  color: 'var(--text-normal)',
                  border: '1px solid var(--background-modifier-border)',
                  borderRadius: 3,
                  fontSize: 13,
                  colorScheme: scheme,
                }}
              >
                {availableThemes.length === 0 && (
                  <option value="">{loaded ? 'No themes installed' : 'Loading...'}</option>
                )}
                {availableThemes.map(t => (
                  <option
                    key={t.id}
                    value={t.id}
                    // Belt-and-braces: also style each option directly.
                    // Chromium honors these on Linux/Windows even when
                    // the popup is OS-native.
                    style={{
                      background: scheme === 'dark' ? '#1f1f1f' : '#ffffff',
                      color: scheme === 'dark' ? '#e5e5e5' : '#1a1a1a',
                    }}
                  >
                    {t.name}
                  </option>
                ))}
              </select>
            )
          })()}
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

      {conflictCount > 0 && (
        <div
          role="status"
          style={{
            padding: 8,
            marginBottom: 12,
            background: 'var(--color-warning-bg, #fff7d6)',
            color: 'var(--color-warning, #8a6d00)',
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
        <table className="keybindings-table" style={{ width: '100%', borderCollapse: 'collapse' }}>
          <thead>
            <tr>
              <th style={cellStyle}>Command</th>
              <th style={cellStyle}>Shortcut</th>
              <th style={{ ...cellStyle, width: 120 }}>Actions</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map(row => (
              <tr key={row.commandId}>
                <td style={cellStyle}>
                  <div style={{ fontWeight: 500 }}>
                    {row.title}
                    {row.overridden && (
                      <span
                        title="Override active"
                        style={{
                          display: 'inline-block',
                          width: 6,
                          height: 6,
                          borderRadius: '50%',
                          background: 'var(--interactive-accent)',
                          marginLeft: 5,
                          verticalAlign: 'middle',
                        }}
                      />
                    )}
                    {row.conflictsWith.length > 0 && (
                      <span
                        title={`Chord conflict — also bound to: ${row.conflictsWith.join(', ')}`}
                        aria-label="Keybinding conflict"
                        style={{
                          display: 'inline-block',
                          marginLeft: 6,
                          padding: '0 5px',
                          fontSize: '0.7em',
                          fontWeight: 600,
                          lineHeight: '14px',
                          color: 'var(--color-warning, #8a6d00)',
                          background: 'var(--color-warning-bg, #fff7d6)',
                          border: '1px solid var(--color-warning, #8a6d00)',
                          borderRadius: 3,
                          verticalAlign: 'middle',
                        }}
                      >
                        {'!'}
                      </span>
                    )}
                  </div>
                  <div style={{ fontSize: '0.85em', opacity: 0.6 }}>{row.commandId}</div>
                </td>
                <td style={cellStyle}>
                  {editing === row.commandId ? (
                    <ChordCaptureInput
                      onCommit={chord => void handleCommit(row.commandId, chord)}
                      onCancel={() => setEditing(null)}
                    />
                  ) : (
                    <div>
                      <code style={{
                        background: row.overridden
                          ? 'var(--interactive-accent-soft, rgba(0,0,0,0.09))'
                          : 'var(--background-modifier-hover, rgba(0,0,0,0.05))',
                        padding: '2px 6px',
                        borderRadius: 3,
                        fontSize: '0.9em',
                        fontWeight: row.overridden ? 600 : undefined,
                      }}>
                        {formatChord(row.current) || '—'}
                      </code>
                      {row.overridden && (
                        <div style={{ marginTop: 3, fontSize: '0.78em', opacity: 0.55 }}>
                          {'← '}{formatChord(row.default) || '—'}
                        </div>
                      )}
                    </div>
                  )}
                </td>
                <td style={{ ...cellStyle, width: 120 }}>
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
  available,
}: {
  corePlugins: PluginInfo[]
  community:   CommunityPluginManifest[]
  available:   AvailablePluginInfo[]
}) {
  const [pendingChanges, setPendingChanges] = useState<Record<string, boolean>>({})
  const [saving,         setSaving]         = useState<string | null>(null)
  const [highRiskOnly,   setHighRiskOnly]   = useState(false)
  // Per-row state for the hot enable/disable flow. `pendingBuiltin`
  // shows a spinner; `builtinErrors` surfaces the failure inline.
  const [pendingBuiltin, setPendingBuiltin] = useState<Set<string>>(new Set())
  const [builtinErrors, setBuiltinErrors] = useState<Record<string, string>>({})

  // The set of built-in plugin ids that ship as default-off — these
  // are the ones that get a toggle in the Core plugins list.
  const optionalIds = useMemo(
    () => new Set(DEFAULT_OFF_PLUGINS.map(e => e.id)),
    [],
  )

  const handleToggleBuiltin = async (pluginId: string, nextEnabled: boolean) => {
    setPendingBuiltin(prev => {
      const next = new Set(prev)
      next.add(pluginId)
      return next
    })
    setBuiltinErrors(prev => {
      if (!(pluginId in prev)) return prev
      const { [pluginId]: _, ...rest } = prev
      return rest
    })
    const result = nextEnabled
      ? await enableBuiltinPlugin(pluginId)
      : await disableBuiltinPlugin(pluginId)
    setPendingBuiltin(prev => {
      const next = new Set(prev)
      next.delete(pluginId)
      return next
    })
    if (!result.ok) {
      setBuiltinErrors(prev => ({ ...prev, [pluginId]: result.error }))
    }
    // Success: pluginActivation refreshes the `pluginList` /
    // `availablePlugins` services and emits PLUGIN_LIST_CHANGED_EVENT,
    // which the parent hooks subscribe to — the row updates itself.
  }

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

  // `highRiskOnly` filters by the parsed capability list. Plugins
  // whose manifest doesn't declare capabilities at all (most of them
  // today) are *not* hidden by the filter — better to show them as
  // "(unknown)" than to silently drop them, since "unknown" is itself
  // a risk signal worth surfacing to the user.
  const filteredCore = useMemo(() => {
    if (!highRiskOnly) return corePlugins
    return corePlugins.filter(p => {
      const caps = parseManifestCapabilities(p.capabilities)
      return caps !== null && hasHighRisk(caps)
    })
  }, [corePlugins, highRiskOnly])

  const filteredCommunity = useMemo(() => {
    if (!highRiskOnly) return community
    return community.filter(m => {
      const caps = parseManifestCapabilities(m.capabilities)
      return caps !== null && hasHighRisk(caps)
    })
  }, [community, highRiskOnly])

  return (
    <div className="plugins-tab">
      {/* Restart banner */}
      {hasPending && (
        <div className="plugins-tab__restart-banner">
          <span>Restart required for changes to take effect.</span>
        </div>
      )}

      {/* High-risk filter — keeps the audit-style "what's spawning
          processes / writing outside the forge?" question one click
          away. Doesn't filter out (unknown)-capability plugins on
          purpose — see filter memo above. */}
      <div
        style={{
          display: 'flex',
          justifyContent: 'flex-end',
          padding: '0 8px 8px 8px',
        }}
      >
        <label
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            fontSize: '0.85em',
            opacity: 0.8,
            cursor: 'pointer',
            userSelect: 'none',
          }}
          title="Show only plugins with at least one high-risk capability"
        >
          <input
            type="checkbox"
            checked={highRiskOnly}
            onChange={e => setHighRiskOnly(e.target.checked)}
          />
          Show only high-risk plugins
        </label>
      </div>

      {/* ── Core plugins ── unified list of loaded built-ins plus the
          dormant default-off ones. Required (default-on) plugins have
          no toggle; optional (default-off) plugins toggle live. */}
      {(() => {
        const optionalDisabled = highRiskOnly ? [] : available
        const totalCore = filteredCore.length + optionalDisabled.length
        return (
          <>
            <div className="plugins-tab__section-header">
              Core plugins
              <span className="plugins-tab__section-count">{totalCore}</span>
              {errorCount > 0 && (
                <span className="plugins-tab__error-badge">{errorCount} error{errorCount > 1 ? 's' : ''}</span>
              )}
            </div>

            <div className="plugins-tab__list">
              {totalCore === 0 ? (
                <p className="settings-empty">
                  {highRiskOnly ? 'No core plugins with high-risk capabilities.' : 'No core plugins loaded.'}
                </p>
              ) : (
                <>
                  {filteredCore.map(p => (
                    <CorePluginRow
                      key={p.id}
                      plugin={p}
                      optional={optionalIds.has(p.id)}
                      busy={pendingBuiltin.has(p.id)}
                      error={builtinErrors[p.id]}
                      onToggle={(next) => { void handleToggleBuiltin(p.id, next) }}
                    />
                  ))}
                  {optionalDisabled.map(p => (
                    <DisabledOptionalRow
                      key={p.id}
                      plugin={p}
                      busy={pendingBuiltin.has(p.id)}
                      error={builtinErrors[p.id]}
                      onToggle={(next) => { void handleToggleBuiltin(p.id, next) }}
                    />
                  ))}
                </>
              )}
            </div>
          </>
        )
      })()}

      {/* ── Community plugins ── */}
      <div className="plugins-tab__section-header" style={{ marginTop: 24 }}>
        Community plugins
        <span className="plugins-tab__section-count">{filteredCommunity.length}</span>
      </div>

      <div className="plugins-tab__list">
        {filteredCommunity.length === 0 ? (
          highRiskOnly ? (
            <p className="settings-empty">No community plugins with high-risk capabilities.</p>
          ) : (
            <div className="plugins-tab__empty-community">
              <p>No community plugins found.</p>
              <p className="plugins-tab__empty-hint">
                Drop a plugin folder into{' '}
                <code>~/.nexus-shell/plugins/</code> then restart.
                Each folder needs a <code>plugin.json</code> and a bundled{' '}
                <code>index.js</code>.
              </p>
            </div>
          )
        ) : (
          filteredCommunity.map(m => (
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

// ─── Core plugin row ─────────────────────────────────────────────────────────
//
// Loaded built-ins. `optional=true` (i.e. shipped via DEFAULT_OFF and
// the user opted-in) gets a toggle in the same style as community
// plugins, so disabling is a single click. Required core plugins
// (DEFAULT_ON) render without a toggle since they're load-bearing.

function CorePluginRow({
  plugin,
  optional,
  busy,
  error,
  onToggle,
}: {
  plugin:   PluginInfo
  optional: boolean
  busy:     boolean
  error?:   string
  onToggle: (next: boolean) => void
}) {
  const capabilities = useMemo(
    () => parseManifestCapabilities(plugin.capabilities),
    [plugin.capabilities],
  )
  return (
    <div className={`plugin-row ${plugin.state === 'error' || error ? 'plugin-row--error' : ''}`}>
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
          {optional && (() => {
            // `pluginList` no longer surfaces 'inactive' rows (they
            // route through DisabledOptionalRow), so anything reaching
            // CorePluginRow is enabled — but bind explicitly rather
            // than hardcoding `checked` so a future regression in the
            // service refresh can't desync the toggle from reality.
            const isEnabled = plugin.state !== 'inactive'
            return (
              <label
                className="plugin-row__toggle"
                title={busy ? 'Working…' : isEnabled ? 'Disable' : 'Enable'}
              >
                <input
                  type="checkbox"
                  checked={isEnabled}
                  disabled={busy}
                  onChange={() => onToggle(!isEnabled)}
                />
                <span className="plugin-row__toggle-track" />
              </label>
            )
          })()}
        </div>
        {plugin.state === 'error' && plugin.error && (
          <div className="plugin-row__error">{plugin.error}</div>
        )}
        {error && <div className="plugin-row__error">{error}</div>}
        <CapabilityChipsRow capabilities={capabilities} />
      </div>
    </div>
  )
}

// ─── Disabled-optional row ───────────────────────────────────────────────────
//
// Default-off built-ins the user hasn't opted into. Renders alongside
// CorePluginRow inside the Core plugins section so the user sees one
// list with toggles in both states (Hello-World style).

function DisabledOptionalRow({
  plugin,
  busy,
  error,
  onToggle,
}: {
  plugin:   AvailablePluginInfo
  busy:     boolean
  error?:   string
  onToggle: (next: boolean) => void
}) {
  return (
    <div className={`plugin-row ${error ? 'plugin-row--error' : ''}`}>
      <div className="plugin-row__dot" data-state={error ? 'error' : 'inactive'} />
      <div className="plugin-row__body">
        <div className="plugin-row__header">
          <span className="plugin-row__name">{plugin.name}</span>
          <span className="plugin-row__id">{plugin.id}</span>
          {plugin.core && (
            <span className="plugin-row__badge plugin-row__badge--core">core</span>
          )}
          <span className="plugin-row__version">v{plugin.version}</span>
          <label
            className="plugin-row__toggle"
            title={busy ? 'Working…' : 'Enable'}
          >
            <input
              type="checkbox"
              checked={false}
              disabled={busy}
              onChange={() => onToggle(true)}
            />
            <span className="plugin-row__toggle-track" />
          </label>
        </div>
        {error && <div className="plugin-row__error">{error}</div>}
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

  // WI-31 wired `CommunityPluginManifest.capabilities` through the Rust
  // scanner, so this is now a real optional field rather than a
  // defensive read.
  const capabilities = useMemo(
    () => parseManifestCapabilities(manifest.capabilities),
    [manifest],
  )

  // WI-31: track HIGH-risk grant count for the subtitle + "Review"
  // button. Loaded lazy on first render; refreshed after a consent
  // modal resolves. A `null` declared count means the manifest omits
  // the capabilities field (no subtitle).
  const declaredHighRiskCount = useMemo(() => {
    if (capabilities === null) return null
    return capabilities.filter(c => CAPABILITY_INFO[c]?.risk === 'high').length
  }, [capabilities])
  const [grantedCount, setGrantedCount] = useState<number>(0)
  const [nonce, setNonce] = useState(0)

  useEffect(() => {
    let cancelled = false
    async function fetch() {
      try {
        const raw = await invoke<Record<string, PriorGrant>>(
          'get_plugin_granted_capabilities',
          { pluginDirs: { [manifest.id]: manifest.dir } },
        )
        if (cancelled) return
        const caps = kernelStringsToCaps(raw[manifest.id]?.capabilities ?? [])
        setGrantedCount(caps.length)
      } catch {
        // best-effort — leave at 0
      }
    }
    void fetch()
    return () => { cancelled = true }
  }, [manifest.id, manifest.dir, nonce])

  const handleReview = async () => {
    if (!capabilities || capabilities.length === 0) return
    let prior: ReturnType<typeof kernelStringsToCaps> = []
    try {
      const raw = await invoke<Record<string, PriorGrant>>(
        'get_plugin_granted_capabilities',
        { pluginDirs: { [manifest.id]: manifest.dir } },
      )
      prior = kernelStringsToCaps(raw[manifest.id]?.capabilities ?? [])
    } catch {
      /* best-effort */
    }
    const result = await requestModalConsent({
      pluginId: manifest.id,
      pluginName: manifest.name,
      version: manifest.version,
      pluginDir: manifest.dir,
      caps: capabilities,
      previouslyGranted: prior,
      reason: 'capability-change',
    })
    try {
      await invoke('set_plugin_granted_capabilities', {
        pluginDir: manifest.dir,
        version: manifest.version,
        capabilities: result === null ? [] : capsToKernelStrings(result),
      })
    } catch (err) {
      console.warn('[settings] set_granted failed:', err)
    }
    setNonce(n => n + 1)
  }

  // WI-33: surface apiVersion mismatch with a red chip and disable the
  // toggle so the user can't try to enable a plugin the host can't load.
  const incompatible = useMemo(() => {
    const declared = manifest.apiVersion
    if (typeof declared !== 'number') return undefined
    if (declared === PLUGIN_API_VERSION) return undefined
    return { requested: declared, supported: PLUGIN_API_VERSION }
  }, [manifest])
  const incompatTitle = incompatible
    ? `Incompatible — requires apiVersion ${incompatible.requested}, ` +
      `shell supports ${incompatible.supported}`
    : undefined

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (incompatible) return
    const next = e.target.checked
    setEnabled(next)
    onToggle(manifest.id, next)
  }

  return (
    <div className="plugin-row">
      <div
        className="plugin-row__dot"
        data-state={incompatible ? 'error' : enabled ? 'active' : 'inactive'}
      />
      <div className="plugin-row__body">
        <div className="plugin-row__header">
          <span className="plugin-row__name">{manifest.name}</span>
          <span className="plugin-row__id">{manifest.id}</span>
          {manifest.author && (
            <span className="plugin-row__author">{manifest.author}</span>
          )}
          <span className="plugin-row__version">v{manifest.version}</span>
          {incompatible && (
            <span
              className="plugin-row__restart-pill"
              title={incompatTitle}
              style={{
                color: 'var(--risk)',
                borderColor: 'var(--risk)',
              }}
            >
              incompatible
            </span>
          )}
          {changed && (
            <span className="plugin-row__restart-pill">restart needed</span>
          )}
          {capabilities && capabilities.length > 0 && !incompatible && (
            <button
              type="button"
              onClick={() => { void handleReview() }}
              title="Review declared capabilities and grants"
              style={{
                padding: '2px 8px',
                background: 'transparent',
                color: 'var(--fg-dim, #888)',
                border: '1px solid var(--color-border, #e0e0e0)',
                borderRadius: 3,
                fontSize: '0.82em',
                cursor: 'pointer',
              }}
            >
              Review
            </button>
          )}
          <label
            className="plugin-row__toggle"
            title={
              incompatible
                ? incompatTitle
                : enabled
                  ? 'Disable'
                  : 'Enable'
            }
          >
            <input
              type="checkbox"
              checked={enabled}
              disabled={saving || !!incompatible}
              onChange={handleChange}
            />
            <span className="plugin-row__toggle-track" />
          </label>
        </div>
        {manifest.description && (
          <div className="plugin-row__description">{manifest.description}</div>
        )}
        {declaredHighRiskCount !== null && declaredHighRiskCount > 0 && (
          <div
            className="plugin-row__description"
            style={{ fontSize: '0.8em', opacity: 0.7 }}
            title={
              `${grantedCount} of ${declaredHighRiskCount} ` +
              'high-risk capabilities granted'
            }
          >
            Granted {grantedCount}/{declaredHighRiskCount} high-risk
          </div>
        )}
        {incompatible && (
          <div
            className="plugin-row__description"
            style={{ color: 'var(--risk)' }}
          >
            Requires plugin API version {incompatible.requested}; this shell
            supports {incompatible.supported}. Update the plugin or the shell
            to match.
          </div>
        )}
        <CapabilityChipsRow capabilities={capabilities} />
      </div>
    </div>
  )
}

// ─── Capability chips (shared by Core + Community rows) ──────────────────────
//
// Mirrors `PluginsMgmtView`'s chip layout but slots into the existing
// `.plugin-row__body` flow — chips render as a row underneath the
// header/description so the row's overall information density stays
// the same.

function CapabilityChipsRow({
  capabilities,
}: {
  capabilities: Capability[] | null
}) {
  if (capabilities === null) {
    return (
      <div style={settingsChipRowStyle}>
        <span
          style={settingsChipMutedStyle}
          title="Plugin manifest does not declare a capabilities list"
        >
          (unknown)
        </span>
      </div>
    )
  }
  if (capabilities.length === 0) {
    return (
      <div style={settingsChipRowStyle}>
        <span style={settingsChipMutedStyle} title="Plugin declared no capabilities">
          (none)
        </span>
      </div>
    )
  }

  const buckets = bucketByRisk(capabilities)
  const ordered: Array<{ risk: RiskLevel; cap: Capability }> = [
    ...buckets.high.map(cap => ({ risk: 'high' as const, cap })),
    ...buckets.medium.map(cap => ({ risk: 'medium' as const, cap })),
    ...buckets.low.map(cap => ({ risk: 'low' as const, cap })),
  ]

  return (
    <div style={settingsChipRowStyle}>
      {ordered.map(({ risk, cap }) => {
        const meta = CAPABILITY_INFO[cap]
        const c = chipColours(risk)
        return (
          <span
            key={cap}
            title={`${risk.toUpperCase()} — ${meta?.description ?? cap}`}
            style={{
              padding: '1px 6px',
              borderRadius: 3,
              background: c.bg,
              color: c.fg,
              border: `1px solid ${c.border}`,
              fontSize: '0.72em',
              fontFamily: 'var(--f-mono, monospace)',
              fontWeight: 500,
              lineHeight: 1.4,
              whiteSpace: 'nowrap',
            }}
          >
            {cap}
          </span>
        )
      })}
    </div>
  )
}

const settingsChipRowStyle: React.CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: 4,
  marginTop: 6,
}

const settingsChipMutedStyle: React.CSSProperties = {
  fontSize: '0.72em',
  fontFamily: 'var(--f-mono, monospace)',
  fontStyle: 'italic',
  opacity: 0.55,
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
