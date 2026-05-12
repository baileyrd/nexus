// src/plugins/core/settings/SettingsPanelView.tsx
// Auto-generates settings UI from registered config schemas.
// Plugins tab: lists core plugins + discovered community plugins with toggles.

import { useState, useEffect, useRef, useCallback, useMemo, createElement, type MouseEvent as ReactMouseEvent } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { PLUGIN_API_VERSION, type Capability } from '@nexus/extension-api'
import { getRegistry } from '../../../host/shellRegistry'
import { useContextKey, useContextKeyStore } from '../../../host/ContextKeyService'
import { useConfigStore, useConfigValue } from '../../../stores/configStore'
import {
  useThemeStore,
  type AvailableSnippet,
} from '../../../stores/themeStore'
import type { ConfigSection, ConfigSchema, PluginAPI, SettingsTabEntry } from '../../../types/plugin'
import type { CommunityPluginManifest } from '../../../host/communityPluginLoader'
import {
  enableBuiltinPlugin,
  disableBuiltinPlugin,
  PLUGIN_LIST_CHANGED_EVENT,
} from '../../../host/pluginActivation'
import { eventBus } from '../../../host/EventBus'
import { clientLogger } from '../../../clientLogger'
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
  kernelStringsToCaps,
  applyCapabilityChange,
  type PriorGrant,
} from '../capabilityPrompt'
import type { SnippetEntry, SnippetConflict } from '../../../registry/SnippetRegistry'

// ─── Types ────────────────────────────────────────────────────────────────────

interface PluginInfo {
  id:           string
  name:         string
  version:      string
  core:         boolean
  state:        string
  error?:       string
  description?: string
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
  id:           string
  name:         string
  version:      string
  core:         boolean
  description?: string
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

// Built-in rail entry ids. Plugin-contributed tab ids and per-plugin
// schema sections (rendered under the "Core plugins" group header) are
// opaque strings. Naming follows Obsidian for parity:
//   general       → about page (version + repo link)
//   appearance    → theme + snippets
//   hotkeys       → keybindings table (was 'keybindings')
//   plugins       → core/community plugin manager
//   snippets      → CSS snippets manager
const BUILT_IN_TABS = [
  'general',
  'editor-options',
  'files-links',
  'appearance',
  'hotkeys',
  'keychain',
  'plugins',
  'community-plugins',
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
  const plugins    = usePluginList()
  const community  = useCommunityManifests()
  const available  = useAvailablePlugins()
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
          <RailItem
            label="Core plugins"
            active={navTab === 'plugins'}
            onClick={() => setNavTab('plugins')}
          />
          <RailItem
            label="Community plugins"
            active={navTab === 'community-plugins'}
            onClick={() => setNavTab('community-plugins')}
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
                <GeneralTab api={api} />
              ) : navTab === 'editor-options' ? (
                <EditorOptionsTab api={api} />
              ) : navTab === 'files-links' ? (
                <FilesLinksTab api={api} />
              ) : navTab === 'appearance' ? (
                <AppearanceTab api={api} />
              ) : navTab === 'hotkeys' ? (
                <KeybindingsTab />
              ) : navTab === 'keychain' ? (
                <KeychainTab api={api} />
              ) : navTab === 'plugins' ? (
                <PluginsTab
                  corePlugins={plugins}
                  community={community}
                  available={available}
                  pluginsWithOptions={
                    new Set([
                      ...sections.map((s) => s.pluginId),
                      ...STUB_CORE_PLUGINS.map((p) => p.id),
                    ])
                  }
                  onJumpToHotkeys={(pluginId) => {
                    // Mirror Obsidian: each row's + button opens the
                    // Hotkeys page pre-filtered to commands owned by
                    // the row's plugin. The seed is consumed once by
                    // KeybindingsTab and then cleared so subsequent
                    // visits start fresh.
                    useContextKeyStore.getState().set('settingsHotkeysQuery', pluginId)
                    setNavTab('hotkeys')
                  }}
                  onJumpToOptions={(pluginId) => {
                    // Mirror Obsidian's gear shortcut on rows whose
                    // plugin has its own settings page.
                    setNavTab(pluginId)
                  }}
                />
              ) : navTab === 'community-plugins' ? (
                <ComingSoonTab
                  title="Community plugins"
                  description="Browse and install third-party plugins from the Nexus directory. For now, drop community plugins into your forge's .forge/plugins/ folder; they'll appear under Core plugins."
                />
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
// Shared pieces for the "Coming soon" pages (General, Editor, Files and
// links, Keychain, Community plugins). Each control fires an info toast
// via `api.notifications` so users get feedback when they poke at a row
// instead of an unresponsive control.

function useComingSoon(api?: PluginAPI) {
  return (label: string) => () => {
    api?.notifications.show({
      type: 'info',
      message: `${label} — coming soon.`,
    })
  }
}

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

function ComingSoonTab({ title, description }: { title: string; description: string }) {
  return (
    <div className="settings-section">
      <div className="settings-section-title">{title}</div>
      <div className="settings-field">
        <div className="settings-field-header">
          <div className="settings-field-title">Coming soon</div>
        </div>
        <div className="settings-field-description">{description}</div>
      </div>
    </div>
  )
}

function GeneralTab({ api }: { api?: PluginAPI }) {
  const version = (import.meta.env?.VITE_APP_VERSION as string | undefined) ?? '0.1.0'
  const comingSoon = useComingSoon(api)

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
          <StubToggle
            on={true}
            label="Toggle automatic updates"
            onClick={comingSoon('Automatic updates')}
          />
        }
      />

      <StubRow
        title="Language"
        description="Change the display language."
        control={
          <select
            defaultValue="en"
            onChange={comingSoon('Language')}
            title="Coming soon"
          >
            <option value="en">English</option>
          </select>
        }
      />

      <StubRow
        title="Help"
        description="Learn how to use Nexus and get help from the community."
        control={
          <button
            type="button"
            onClick={comingSoon('Help')}
            title="Coming soon"
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
        title="Edit settings file"
        description="Open .forge/app.toml in a new editor tab. Direct edits to the [settings] table take effect after closing and reopening the forge."
        control={
          <button
            type="button"
            onClick={() => {
              eventBus.emit('files:open', {
                relpath: '.forge/app.toml',
                name: 'app.toml',
              })
              useContextKeyStore.getState().set('settingsPanelVisible', false)
            }}
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

      <StubRow
        title="Notify if startup takes longer than expected"
        description="Diagnose issues by seeing what is causing the app to load slowly."
        control={
          <StubToggle
            on={false}
            label="Toggle slow-startup notification"
            onClick={comingSoon('Startup-time notification')}
          />
        }
      />

      <StubRow
        title="Command line interface"
        description="Allow interactions with Nexus from the command line."
        control={
          <StubToggle
            on={false}
            label="Toggle command line interface"
            onClick={comingSoon('Command line interface')}
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

function EditorOptionsTab({ api }: { api?: PluginAPI }) {
  const comingSoon = useComingSoon(api)
  return (
    <div className="settings-section">
      <StubRow
        title="Always focus new tabs"
        description="When you open a link in a new tab, switch to it immediately."
        control={
          <StubToggle on={true} label="Toggle focus new tabs" onClick={comingSoon('Always focus new tabs')} />
        }
      />
      <StubRow
        title="Default view for new tabs"
        description="The default view that a new Markdown tab gets opened in."
        control={
          <select defaultValue="editing" onChange={comingSoon('Default view for new tabs')} title="Coming soon">
            <option value="editing">Editing view</option>
            <option value="reading">Reading view</option>
          </select>
        }
      />
      <StubRow
        title="Default editing mode"
        description="The default editing mode a new tab will start with."
        control={
          <select defaultValue="live" onChange={comingSoon('Default editing mode')} title="Coming soon">
            <option value="live">Live Preview</option>
            <option value="source">Source mode</option>
          </select>
        }
      />
      <StubRow
        title="Show editing mode in status bar"
        description="Show the editing mode toggle in the status bar."
        control={
          <StubToggle
            on={true}
            label="Toggle editing-mode status bar"
            onClick={comingSoon('Show editing mode in status bar')}
          />
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Display</div>

      <StubRow
        title="Readable line length"
        description="Limit maximum line length. Less content fits onscreen, but long blocks of text are more readable."
        control={
          <StubToggle on={true} label="Toggle readable line length" onClick={comingSoon('Readable line length')} />
        }
      />
      <StubRow
        title="Strict line breaks"
        description="Markdown specs ignore single line breaks in reading view. Turn this off to make single line breaks visible."
        control={
          <StubToggle on={false} label="Toggle strict line breaks" onClick={comingSoon('Strict line breaks')} />
        }
      />
      <StubRow
        title="Properties in document"
        description="Choose how properties are displayed at the top of notes. Select &ldquo;source&rdquo; to show properties as raw YAML."
        control={
          <select defaultValue="visible" onChange={comingSoon('Properties in document')} title="Coming soon">
            <option value="visible">Visible</option>
            <option value="hidden">Hidden</option>
            <option value="source">Source</option>
          </select>
        }
      />
      <StubRow
        title="Fold heading"
        description="Lets you fold all content under a heading."
        control={<StubToggle on={true} label="Toggle fold heading" onClick={comingSoon('Fold heading')} />}
      />
      <StubRow
        title="Fold indent"
        description="Lets you fold part of an indentation, such as lists."
        control={<StubToggle on={true} label="Toggle fold indent" onClick={comingSoon('Fold indent')} />}
      />
      <StubRow
        title="Line numbers"
        description="Show line numbers in the gutter."
        control={<StubToggle on={false} label="Toggle line numbers" onClick={comingSoon('Line numbers')} />}
      />
      <StubRow
        title="Indentation guides"
        description="Show vertical relationship lines between list items."
        control={
          <StubToggle on={true} label="Toggle indentation guides" onClick={comingSoon('Indentation guides')} />
        }
      />
      <StubRow
        title="Right-to-left (RTL)"
        description="Sets the default text direction of notes to right-to-left."
        control={<StubToggle on={false} label="Toggle RTL" onClick={comingSoon('Right-to-left (RTL)')} />}
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Behavior</div>

      <StubRow
        title="Spellcheck"
        description="Turn on the spellchecker."
        control={
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <button
              type="button"
              onClick={comingSoon('Spellcheck options')}
              title="Coming soon"
              aria-label="Spellcheck options"
              style={{
                background: 'transparent',
                border: 'none',
                color: 'var(--text-muted)',
                cursor: 'pointer',
                padding: 2,
                fontSize: 14,
                lineHeight: 1,
              }}
            >
              ⚙
            </button>
            <StubToggle on={true} label="Toggle spellcheck" onClick={comingSoon('Spellcheck')} />
          </div>
        }
      />
      <StubRow
        title="Spellcheck languages"
        description="Choose the languages for the spellchecker to use."
        control={
          <select defaultValue="en-US" onChange={comingSoon('Spellcheck languages')} title="Coming soon">
            <option value="en-US">English (United States)</option>
            <option value="add">+ Add language…</option>
          </select>
        }
      />
      <StubRow
        title="Auto-pair brackets"
        description="Pair brackets and quotes automatically."
        control={<StubToggle on={true} label="Toggle auto-pair brackets" onClick={comingSoon('Auto-pair brackets')} />}
      />
      <StubRow
        title="Auto-pair Markdown syntax"
        description="Pair symbols automatically for bold, italic, code, and more."
        control={
          <StubToggle
            on={true}
            label="Toggle auto-pair Markdown syntax"
            onClick={comingSoon('Auto-pair Markdown syntax')}
          />
        }
      />
      <StubRow
        title="Smart lists"
        description="Automatically set indentation and place list items correctly."
        control={<StubToggle on={true} label="Toggle smart lists" onClick={comingSoon('Smart lists')} />}
      />
      <StubRow
        title="Indent using tabs"
        description="Use tabs to indent by pressing the &ldquo;Tab&rdquo; key. Turn this off to indent using 4 spaces."
        control={<StubToggle on={true} label="Toggle indent using tabs" onClick={comingSoon('Indent using tabs')} />}
      />
      <StubRow
        title="Indent visual width"
        description="Number of spaces a tab character will render as."
        control={
          <input
            type="range"
            min={2}
            max={8}
            defaultValue={4}
            onChange={comingSoon('Indent visual width')}
            title="Coming soon"
            style={{ minWidth: 120 }}
          />
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Advanced</div>

      <StubRow
        title="Convert pasted HTML to Markdown"
        description="Automatically convert HTML to Markdown when pasting and drag-and-drop from web pages. Use Ctrl/Cmd+Shift+V to paste HTML without converting."
        control={
          <StubToggle
            on={true}
            label="Toggle convert pasted HTML"
            onClick={comingSoon('Convert pasted HTML to Markdown')}
          />
        }
      />
      <StubRow
        title="Vim key bindings"
        description="Use Vim key bindings when editing."
        control={<StubToggle on={false} label="Toggle Vim key bindings" onClick={comingSoon('Vim key bindings')} />}
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
  const comingSoon = useComingSoon(api)
  return (
    <div className="settings-section">
      <StubRow
        title="Default file to open"
        description="Choose which file to open when the app starts."
        control={
          <select defaultValue="last" onChange={comingSoon('Default file to open')} title="Coming soon">
            <option value="last">Last opened</option>
            <option value="none">None</option>
            <option value="specific">Specific file…</option>
          </select>
        }
      />
      <StubRow
        title="Default location for new notes"
        description="Where newly created notes are placed."
        control={
          <select defaultValue="root" onChange={comingSoon('Default location for new notes')} title="Coming soon">
            <option value="root">Forge folder</option>
            <option value="same">Same folder as current file</option>
            <option value="specific">Specific folder…</option>
          </select>
        }
      />
      <StubRow
        title="Default location for new attachments"
        description="Where newly added attachments are placed."
        control={
          <select
            defaultValue="root"
            onChange={comingSoon('Default location for new attachments')}
            title="Coming soon"
          >
            <option value="root">Forge folder</option>
            <option value="same">Same folder as current file</option>
            <option value="specific">Specific folder…</option>
          </select>
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Links</div>

      <StubRow
        title="New link format"
        description="What links to insert when auto-generating internal links."
        control={
          <select defaultValue="shortest" onChange={comingSoon('New link format')} title="Coming soon">
            <option value="shortest">Shortest path when possible</option>
            <option value="relative">Relative path</option>
            <option value="absolute">Absolute path</option>
          </select>
        }
      />
      <StubRow
        title="Automatically update internal links"
        description="Turn off to be prompted to update links after renaming a file."
        control={
          <StubToggle
            on={false}
            label="Toggle automatic link updates"
            onClick={comingSoon('Automatically update internal links')}
          />
        }
      />
      <StubRow
        title="Use [[Wikilinks]]"
        description="Auto-generate Wikilinks for [[links]] and ![[images]] instead of Markdown links and images. Disable this option to generate Markdown links instead."
        control={<StubToggle on={true} label="Toggle wikilinks" onClick={comingSoon('Use Wikilinks')} />}
      />
      <StubRow
        title="Show all file types"
        description="Show files with any extension even if Nexus can't open them natively, so you can link to them and see them in the file explorer and quick switcher."
        control={
          <StubToggle on={false} label="Toggle show all file types" onClick={comingSoon('Show all file types')} />
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Trash</div>

      <StubRow
        title="Confirm before deleting files"
        description="Avoid accidentally deleting files."
        control={
          <StubToggle on={true} label="Toggle delete confirmation" onClick={comingSoon('Confirm before deleting files')} />
        }
      />
      <StubRow
        title="Delete attachments when deleting files"
        description="Automatically remove attachments linked to the deleted file if they're not used elsewhere."
        control={
          <select
            defaultValue="ask"
            onChange={comingSoon('Delete attachments when deleting files')}
            title="Coming soon"
          >
            <option value="ask">Ask each time</option>
            <option value="always">Always</option>
            <option value="never">Never</option>
          </select>
        }
      />
      <StubRow
        title="Deleted files"
        description="What happens to a file after you delete it."
        control={
          <select defaultValue="system" onChange={comingSoon('Deleted files')} title="Coming soon">
            <option value="system">Move to system trash</option>
            <option value="forge">Move to .trash in forge</option>
            <option value="permanent">Delete permanently</option>
          </select>
        }
      />

      <div className="settings-section-title" style={{ marginTop: 24 }}>Advanced</div>

      <StubRow
        title="Excluded files"
        description="Excluded files will be hidden in Search, Graph view, and Unlinked Mentions, less noticeable in Quick Switcher and link suggestions."
        control={
          <button
            type="button"
            onClick={comingSoon('Excluded files')}
            title="Coming soon"
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
            Manage
          </button>
        }
      />
      <StubRow
        title="Override config folder"
        description="Use a different config folder than the default one. Must start with a dot."
        control={
          <input
            type="text"
            placeholder=".forge"
            onChange={comingSoon('Override config folder')}
            title="Coming soon"
            style={{ minWidth: 180 }}
          />
        }
      />
      <StubRow
        title="Allow URI callbacks"
        description="Enable the use of x-callback-url through x-success or x-error when handling Nexus URIs."
        control={<StubToggle on={false} label="Toggle URI callbacks" onClick={comingSoon('Allow URI callbacks')} />}
      />
      <StubRow
        title="Rebuild forge cache"
        description="Rebuilding the cache could take a few seconds to a few minutes depending on the size of your forge."
        control={
          <button
            type="button"
            onClick={comingSoon('Rebuild forge cache')}
            title="Coming soon"
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
  const comingSoon = useComingSoon(api)
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
          onClick={comingSoon('Add secret')}
          title="Coming soon"
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
    render: (api) => <StubSyncPage api={api} />,
  },
  {
    id: 'cp-stub:templates',
    label: 'Templates',
    render: (api) => <StubTemplatesPage api={api} />,
  },
]

const STUB_CORE_BY_ID = new Map(STUB_CORE_PLUGINS.map((p) => [p.id, p]))

function StubBacklinksPage({ api }: { api?: PluginAPI }) {
  const comingSoon = useComingSoon(api)
  return (
    <div className="settings-section">
      <StubRow
        title="Show backlinks at the bottom of notes"
        description="Make backlinks visible in new tabs by default."
        control={
          <StubToggle on={false} label="Toggle backlinks at bottom" onClick={comingSoon('Show backlinks at the bottom of notes')} />
        }
      />
    </div>
  )
}

function StubCanvasPage({ api }: { api?: PluginAPI }) {
  const comingSoon = useComingSoon(api)
  return (
    <div className="settings-section">
      <StubRow
        title="Default location for new canvas files"
        description="Where newly created canvases are placed."
        control={
          <select defaultValue="root" onChange={comingSoon('Default canvas location')} title="Coming soon">
            <option value="root">Forge folder</option>
            <option value="same">Same folder as current file</option>
            <option value="specific">Specific folder…</option>
          </select>
        }
      />
      <StubRow
        title="Default mouse wheel behavior"
        description=""
        control={
          <select defaultValue="pan" onChange={comingSoon('Default mouse wheel behavior')} title="Coming soon">
            <option value="pan">Pan</option>
            <option value="zoom">Zoom</option>
          </select>
        }
      />
      <StubRow
        title="Default Ctrl + Drag behavior"
        description=""
        control={
          <select defaultValue="menu" onChange={comingSoon('Default Ctrl+Drag behavior')} title="Coming soon">
            <option value="menu">Show menu</option>
            <option value="select">Select</option>
            <option value="zoom">Zoom</option>
          </select>
        }
      />
      <StubRow
        title="Show card names"
        description=""
        control={
          <select defaultValue="always" onChange={comingSoon('Show card names')} title="Coming soon">
            <option value="always">Always</option>
            <option value="hover">On hover</option>
            <option value="never">Never</option>
          </select>
        }
      />
      <StubRow
        title="Snap to grid"
        description="Snap cards to the background grid when moving and resizing."
        control={<StubToggle on={true} label="Toggle snap to grid" onClick={comingSoon('Snap to grid')} />}
      />
      <StubRow
        title="Snap to objects"
        description="Snap cards to nearby objects when moving and resizing."
        control={<StubToggle on={true} label="Toggle snap to objects" onClick={comingSoon('Snap to objects')} />}
      />
      <StubRow
        title="Zoom threshold for hiding card content"
        description="Lower values will increase performance but hide card content sooner when zooming out."
        control={
          <input
            type="range"
            min={0}
            max={100}
            defaultValue={40}
            onChange={comingSoon('Zoom threshold for hiding card content')}
            title="Coming soon"
            style={{ minWidth: 120 }}
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

function StubDailyNotesPage({ api }: { api?: PluginAPI }) {
  const comingSoon = useComingSoon(api)
  const today = new Date().toISOString().slice(0, 10)
  return (
    <div className="settings-section">
      <StubRow
        title="Date format"
        description="Choose how daily notes are named in your forge."
        control={
          <input
            type="text"
            defaultValue={today}
            onChange={comingSoon('Date format')}
            title="Coming soon"
            style={{ minWidth: 160 }}
          />
        }
      />
      <StubRow
        title="New file location"
        description="New daily notes will be placed here."
        control={
          <input
            type="text"
            placeholder="Example: folder 1/folder 2"
            onChange={comingSoon('Daily note location')}
            title="Coming soon"
            style={{ minWidth: 200 }}
          />
        }
      />
      <StubRow
        title="Template file location"
        description="Choose the file to use as a template."
        control={
          <input
            type="text"
            placeholder="Example: folder/note"
            onChange={comingSoon('Daily note template')}
            title="Coming soon"
            style={{ minWidth: 200 }}
          />
        }
      />
    </div>
  )
}

function StubFileRecoveryPage({ api }: { api?: PluginAPI }) {
  const comingSoon = useComingSoon(api)
  return (
    <div className="settings-section">
      <StubRow
        title="Snapshot interval"
        description="Minimal interval in minutes between two snapshots."
        control={
          <input
            type="number"
            min={1}
            defaultValue={5}
            onChange={comingSoon('Snapshot interval')}
            title="Coming soon"
            style={{ width: 80 }}
          />
        }
      />
      <StubRow
        title="History length"
        description="Number of days the snapshots are kept for."
        control={
          <input
            type="number"
            min={1}
            defaultValue={7}
            onChange={comingSoon('History length')}
            title="Coming soon"
            style={{ width: 80 }}
          />
        }
      />
      <StubRow
        title="Snapshots"
        description="View and restore saved snapshots."
        control={
          <button
            type="button"
            onClick={comingSoon('View snapshots')}
            title="Coming soon"
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
            onClick={comingSoon('Clear file recovery history')}
            title="Coming soon"
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

function StubNoteComposerPage({ api }: { api?: PluginAPI }) {
  const comingSoon = useComingSoon(api)
  return (
    <div className="settings-section">
      <StubRow
        title="Text after extraction"
        description="What to show in place of the selected text after extracting it."
        control={
          <select defaultValue="link" onChange={comingSoon('Text after extraction')} title="Coming soon">
            <option value="link">Link to new file</option>
            <option value="embed">Embed new file</option>
            <option value="nothing">Nothing</option>
          </select>
        }
      />
      <StubRow
        title="Template file location"
        description="Template file to use when merging or extracting. Available variables: {{content}}, {{fromTitle}}, {{newTitle}}, {{date:FORMAT}}, e.g. {{date:YYYY-MM-DD}}."
        control={
          <input
            type="text"
            placeholder="Example: folder/note"
            onChange={comingSoon('Note composer template')}
            title="Coming soon"
            style={{ minWidth: 200 }}
          />
        }
      />
      <StubRow
        title="Confirm file merge"
        description="Prompt before merging two files."
        control={<StubToggle on={true} label="Toggle confirm file merge" onClick={comingSoon('Confirm file merge')} />}
      />
    </div>
  )
}

function StubPagePreviewPage({ api }: { api?: PluginAPI }) {
  const comingSoon = useComingSoon(api)
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
            <StubToggle
              on={s.on}
              label={`Toggle Ctrl-required on ${s.label}`}
              onClick={comingSoon(`Page preview: ${s.label}`)}
            />
          }
        />
      ))}
    </div>
  )
}

function StubQuickSwitcherPage({ api }: { api?: PluginAPI }) {
  const comingSoon = useComingSoon(api)
  return (
    <div className="settings-section">
      <StubRow
        title="Show existing only"
        description="Only show results from existing files. Links to files that are not yet created will be hidden."
        control={
          <StubToggle on={false} label="Toggle show existing only" onClick={comingSoon('Show existing only')} />
        }
      />
      <StubRow
        title="Show attachments"
        description="Show attachment files like images, videos, and PDFs."
        control={<StubToggle on={true} label="Toggle show attachments" onClick={comingSoon('Show attachments')} />}
      />
    </div>
  )
}

function StubSyncPage({ api }: { api?: PluginAPI }) {
  const comingSoon = useComingSoon(api)
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
          onClick={comingSoon('Sign up for Sync')}
          title="Coming soon"
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
          onClick={comingSoon('Log in to Sync')}
          title="Coming soon"
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

function StubTemplatesPage({ api }: { api?: PluginAPI }) {
  const comingSoon = useComingSoon(api)
  const now = new Date()
  const today = now.toISOString().slice(0, 10)
  const time = now.toTimeString().slice(0, 5)
  return (
    <div className="settings-section">
      <StubRow
        title="Template folder location"
        description="Files in this folder will be available as templates."
        control={
          <input
            type="text"
            placeholder="Example: folder 1/folder 2"
            onChange={comingSoon('Template folder location')}
            title="Coming soon"
            style={{ minWidth: 200 }}
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
          <input
            type="text"
            placeholder="YYYY-MM-DD"
            onChange={comingSoon('Templates date format')}
            title="Coming soon"
            style={{ minWidth: 160 }}
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
          <input
            type="text"
            placeholder="HH:mm"
            onChange={comingSoon('Templates time format')}
            title="Coming soon"
            style={{ minWidth: 160 }}
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

  const comingSoon = useComingSoon(api)

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
          <button
            type="button"
            onClick={comingSoon('Accent color')}
            title="Coming soon"
            aria-label="Pick accent color"
            style={{
              width: 22,
              height: 22,
              borderRadius: '50%',
              background: 'var(--interactive-accent)',
              border: '1px solid var(--background-modifier-border)',
              cursor: 'pointer',
              padding: 0,
            }}
          />
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
              onClick={comingSoon('Manage themes')}
              title="Coming soon"
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
        control={<StubToggle on={true} label="Toggle inline title" onClick={comingSoon('Inline title')} />}
      />
      <StubRow
        title="Show tab title bar"
        description="Display the header at the top of every tab."
        control={<StubToggle on={true} label="Toggle tab title bar" onClick={comingSoon('Show tab title bar')} />}
      />
      <StubRow
        title="Show ribbon"
        description="Display vertical toolbar on the side of the window."
        control={<StubToggle on={true} label="Toggle ribbon" onClick={comingSoon('Show ribbon')} />}
      />
      <StubRow
        title="Ribbon menu configuration"
        description="Configure what commands appear in the ribbon menu."
        control={
          <button
            type="button"
            onClick={comingSoon('Ribbon menu configuration')}
            title="Coming soon"
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
            Manage
          </button>
        }
      />

      {/* ── Font (stubs) ──────────────────────────────────────── */}
      <div className="settings-section-title" style={{ marginTop: 24 }}>Font</div>
      <StubRow
        title="Interface font"
        description="Set base font for all of Nexus."
        control={
          <button
            type="button"
            onClick={comingSoon('Interface font')}
            title="Coming soon"
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
            Manage
          </button>
        }
      />
      <StubRow
        title="Text font"
        description="Set font for editing and reading views."
        control={
          <button
            type="button"
            onClick={comingSoon('Text font')}
            title="Coming soon"
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
            Manage
          </button>
        }
      />
      <StubRow
        title="Monospace font"
        description="Set font for places like code blocks and frontmatter."
        control={
          <button
            type="button"
            onClick={comingSoon('Monospace font')}
            title="Coming soon"
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
            Manage
          </button>
        }
      />
      <StubRow
        title="Font size"
        description="Font size in pixels that affects editing and reading views."
        control={
          <input
            type="range"
            min={10}
            max={24}
            defaultValue={14}
            onChange={comingSoon('Font size')}
            title="Coming soon"
            style={{ minWidth: 120 }}
          />
        }
      />
      <StubRow
        title="Quick font size adjustment"
        description="Adjust the font size using Ctrl + Scroll, or using the trackpad pinch-zoom gesture."
        control={
          <StubToggle
            on={false}
            label="Toggle quick font size adjustment"
            onClick={comingSoon('Quick font size adjustment')}
          />
        }
      />

      {/* ── Advanced (stubs) ──────────────────────────────────── */}
      <div className="settings-section-title" style={{ marginTop: 24 }}>Advanced</div>
      <StubRow
        title="Zoom level"
        description="Controls the overall zoom level of the app."
        control={
          <input
            type="range"
            min={50}
            max={200}
            defaultValue={100}
            onChange={comingSoon('Zoom level')}
            title="Coming soon"
            style={{ minWidth: 120 }}
          />
        }
      />
      <StubRow
        title="Native menus"
        description="Menus throughout the app will match the operating system. They will not be affected by your theme."
        control={<StubToggle on={false} label="Toggle native menus" onClick={comingSoon('Native menus')} />}
      />
      <StubRow
        title="Window frame style"
        description="Determines the styling of the title bar of Nexus windows. Requires a full restart to take effect."
        control={
          <select defaultValue="hidden" onChange={comingSoon('Window frame style')} title="Coming soon">
            <option value="hidden">Hidden (default)</option>
            <option value="native">Native</option>
            <option value="custom">Custom</option>
          </select>
        }
      />
      <StubRow
        title="Custom app icon"
        description="Set a custom icon for the app."
        control={
          <button
            type="button"
            onClick={comingSoon('Custom app icon')}
            title="Coming soon"
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
        }
      />
      <StubRow
        title="Hardware acceleration"
        description={
          'Turns on hardware acceleration, which uses your GPU to make Nexus smoother. ' +
          'If you turn this off, app performance will be severely degraded.'
        }
        control={
          <StubToggle
            on={true}
            label="Toggle hardware acceleration"
            onClick={comingSoon('Hardware acceleration')}
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

// ─── Plugins tab ──────────────────────────────────────────────────────────────

function PluginsTab({
  corePlugins,
  community,
  available,
  pluginsWithOptions,
  onJumpToHotkeys,
  onJumpToOptions,
}: {
  corePlugins: PluginInfo[]
  community:   CommunityPluginManifest[]
  available:   AvailablePluginInfo[]
  pluginsWithOptions: Set<string>
  onJumpToHotkeys: (pluginId: string) => void
  onJumpToOptions: (pluginId: string) => void
}) {
  const [pendingChanges, setPendingChanges] = useState<Record<string, boolean>>({})
  const [saving,         setSaving]         = useState<string | null>(null)
  const [highRiskOnly,   setHighRiskOnly]   = useState(false)
  const [pluginQuery,    setPluginQuery]    = useState('')
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
      clientLogger.error('[PluginsTab] set_plugin_enabled failed:', err)
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
  // Two-stage filter: high-risk gate first, then text query against
  // name + id + description. The query field at the top of the page
  // mirrors Obsidian's "Search plugins..." header pattern.
  const matchesQuery = useCallback(
    (fields: ReadonlyArray<string | undefined>) => {
      const q = pluginQuery.trim().toLowerCase()
      if (!q) return true
      return fields.some((v) => typeof v === 'string' && v.toLowerCase().includes(q))
    },
    [pluginQuery],
  )

  const filteredCore = useMemo(() => {
    return corePlugins.filter((p) => {
      if (highRiskOnly) {
        const caps = parseManifestCapabilities(p.capabilities)
        if (caps === null || !hasHighRisk(caps)) return false
      }
      return matchesQuery([p.name, p.id, p.description])
    })
  }, [corePlugins, highRiskOnly, matchesQuery])

  const filteredAvailable = useMemo(
    () => available.filter((p) => matchesQuery([p.name, p.id, p.description])),
    [available, matchesQuery],
  )

  const filteredCommunity = useMemo(() => {
    return community.filter((m) => {
      if (highRiskOnly) {
        const caps = parseManifestCapabilities(m.capabilities)
        if (caps === null || !hasHighRisk(caps)) return false
      }
      return matchesQuery([m.name, m.id, m.description])
    })
  }, [community, highRiskOnly, matchesQuery])

  return (
    <div className="plugins-tab">
      {/* Restart banner */}
      {hasPending && (
        <div className="plugins-tab__restart-banner">
          <span>Restart required for changes to take effect.</span>
        </div>
      )}

      {/* Header — Obsidian-style search input + audit-mode toggle.
          Keeps the high-risk filter one click away while making the
          search box the primary affordance. */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 12 }}>
        <input
          type="search"
          className="settings-search"
          placeholder="Search plugins..."
          value={pluginQuery}
          onChange={(e) => setPluginQuery(e.target.value)}
          style={{ flex: 1 }}
        />
        <label
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            fontSize: '0.85em',
            opacity: 0.8,
            cursor: 'pointer',
            userSelect: 'none',
            whiteSpace: 'nowrap',
          }}
          title="Show only plugins with at least one high-risk capability"
        >
          <input
            type="checkbox"
            checked={highRiskOnly}
            onChange={(e) => setHighRiskOnly(e.target.checked)}
          />
          High-risk only
        </label>
      </div>

      {/* ── Core plugins ── unified list of loaded built-ins plus the
          dormant default-off ones. Required (default-on) plugins have
          no toggle; optional (default-off) plugins toggle live. */}
      {(() => {
        const optionalDisabled = highRiskOnly ? [] : filteredAvailable
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
                      onJumpToHotkeys={() => onJumpToHotkeys(p.id)}
                      hasOptions={pluginsWithOptions.has(p.id)}
                      onJumpToOptions={() => onJumpToOptions(p.id)}
                    />
                  ))}
                  {optionalDisabled.map(p => (
                    <DisabledOptionalRow
                      key={p.id}
                      plugin={p}
                      busy={pendingBuiltin.has(p.id)}
                      error={builtinErrors[p.id]}
                      onToggle={(next) => { void handleToggleBuiltin(p.id, next) }}
                      onJumpToHotkeys={() => onJumpToHotkeys(p.id)}
                      hasOptions={pluginsWithOptions.has(p.id)}
                      onJumpToOptions={() => onJumpToOptions(p.id)}
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
              onJumpToHotkeys={() => onJumpToHotkeys(m.id)}
              hasOptions={pluginsWithOptions.has(m.id)}
              onJumpToOptions={() => onJumpToOptions(m.id)}
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
  onJumpToHotkeys,
  hasOptions,
  onJumpToOptions,
}: {
  plugin:   PluginInfo
  optional: boolean
  busy:     boolean
  error?:   string
  onToggle: (next: boolean) => void
  onJumpToHotkeys: () => void
  hasOptions: boolean
  onJumpToOptions: () => void
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
          {(() => {
            // 'registered' is the kernel's lazy-activation state — the plugin
            // is loaded and its triggers wired, but activate() runs on first
            // use. From the user's POV that's just "ready", so map the label.
            const display = plugin.state === 'registered' ? 'ready' : plugin.state
            return (
              <span className={`plugin-row__state plugin-row__state--${display}`}>
                {display}
              </span>
            )
          })()}
          {hasOptions && <OptionsShortcutButton onClick={onJumpToOptions} />}
          <HotkeysShortcutButton onClick={onJumpToHotkeys} />
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
        {plugin.description && (
          <div className="plugin-row__description">{plugin.description}</div>
        )}
        {plugin.state === 'error' && plugin.error && (
          <div className="plugin-row__error">{plugin.error}</div>
        )}
        {error && <div className="plugin-row__error">{error}</div>}
        {/* Core plugins inherit Capability::ALL from bootstrap and don't
            declare per-plugin capabilities, so an "(unknown)" chip would
            be noise. Render the chip row only when the manifest actually
            lists something. */}
        {capabilities !== null && capabilities.length > 0 && (
          <CapabilityChipsRow capabilities={capabilities} />
        )}
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
  onJumpToHotkeys,
  hasOptions,
  onJumpToOptions,
}: {
  plugin:   AvailablePluginInfo
  busy:     boolean
  error?:   string
  onToggle: (next: boolean) => void
  onJumpToHotkeys: () => void
  hasOptions: boolean
  onJumpToOptions: () => void
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
          {hasOptions && <OptionsShortcutButton onClick={onJumpToOptions} />}
          <HotkeysShortcutButton onClick={onJumpToHotkeys} />
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
        {plugin.description && (
          <div className="plugin-row__description">{plugin.description}</div>
        )}
        {error && <div className="plugin-row__error">{error}</div>}
      </div>
    </div>
  )
}

// Compact gear button next to a plugin row's toggle. Only rendered
// when the plugin has its own settings page (real config schema or a
// cp-stub:* placeholder). Clicking jumps the rail to that plugin's
// settings entry — matches Obsidian's row-level "Options" shortcut.
function OptionsShortcutButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title="Options"
      aria-label="Open settings for this plugin"
      style={{
        width: 22,
        height: 22,
        marginRight: 6,
        borderRadius: '50%',
        border: '1px solid var(--background-modifier-border)',
        background: 'transparent',
        color: 'var(--text-muted)',
        cursor: 'pointer',
        display: 'inline-grid',
        placeItems: 'center',
        fontSize: 12,
        lineHeight: 1,
        padding: 0,
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = 'var(--background-modifier-hover)'
        e.currentTarget.style.color = 'var(--text-normal)'
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = 'transparent'
        e.currentTarget.style.color = 'var(--text-muted)'
      }}
    >
      ⚙
    </button>
  )
}

// Compact `+` button next to a plugin row's toggle. Mirrors Obsidian's
// "open Hotkeys filtered to this plugin" affordance — clicking
// switches the settings panel to the Hotkeys page with a search query
// pre-filled with the plugin id, so only that plugin's commands show.
function HotkeysShortcutButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title="Hotkeys"
      aria-label="Open hotkeys for this plugin"
      style={{
        width: 22,
        height: 22,
        marginRight: 6,
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
      onMouseEnter={(e) => {
        e.currentTarget.style.background = 'var(--background-modifier-hover)'
        e.currentTarget.style.color = 'var(--text-normal)'
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = 'transparent'
        e.currentTarget.style.color = 'var(--text-muted)'
      }}
    >
      +
    </button>
  )
}

// ─── Community plugin row (toggleable) ───────────────────────────────────────

function CommunityPluginRow({
  manifest, saving, changed, onToggle, onJumpToHotkeys, hasOptions, onJumpToOptions,
}: {
  manifest: CommunityPluginManifest
  saving:   boolean
  changed:  boolean
  onToggle: (id: string, enabled: boolean) => void
  onJumpToHotkeys: () => void
  hasOptions: boolean
  onJumpToOptions: () => void
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
      // BL-096 follow-up — applyCapabilityChange persists the new
      // set AND issues `revoke_plugin_capability` for any cap that
      // was previously granted but is no longer in `result`. Live
      // revoke means the running plugin loses access immediately;
      // pre-fix, the disk write only took effect at next boot.
      const { revokeErrors } = await applyCapabilityChange(
        { invoke: invoke as never },
        {
          pluginId: manifest.id,
          pluginDir: manifest.dir,
          version: manifest.version,
          prior,
          next: result,
        },
      )
      for (const { capability, error } of revokeErrors) {
        clientLogger.warn(
          `[settings] live-revoke failed for ${capability}:`,
          error,
        )
      }
    } catch (err) {
      clientLogger.warn('[settings] set_granted failed:', err)
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
          {/* OI-15 — verification badge */}
          {(manifest.verificationStatus === 'verified')
            ? (
              <span
                className="plugin-row__restart-pill"
                title="Signed by a trusted key"
                style={{ color: 'var(--nexus-color-success)', borderColor: 'var(--nexus-color-success)' }}
              >
                verified
              </span>
            )
            : (
              <span
                className="plugin-row__restart-pill"
                title="No trusted signature — install only plugins you trust"
                style={{ color: 'var(--text-faint)', borderColor: 'var(--background-modifier-border)' }}
              >
                unsigned
              </span>
            )
          }
          {capabilities && capabilities.length > 0 && !incompatible && (
            <button
              type="button"
              onClick={() => { void handleReview() }}
              title="Review declared capabilities and grants"
              style={{
                padding: '2px 8px',
                background: 'transparent',
                color: 'var(--text-faint)',
                border: '1px solid var(--background-modifier-border)',
                borderRadius: 3,
                fontSize: '0.82em',
                cursor: 'pointer',
              }}
            >
              Review
            </button>
          )}
          {hasOptions && <OptionsShortcutButton onClick={onJumpToOptions} />}
          <HotkeysShortcutButton onClick={onJumpToHotkeys} />
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
              fontFamily: 'var(--font-monospace, monospace)',
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
  fontFamily: 'var(--font-monospace, monospace)',
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
