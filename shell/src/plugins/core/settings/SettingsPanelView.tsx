// src/plugins/core/settings/SettingsPanelView.tsx
// Auto-generates settings UI from registered config schemas.
// Plugins tab: lists core plugins + discovered community plugins with toggles.

import { useState, useEffect, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { getRegistry } from '../../../host/shellRegistry'
import { useContextKey, useContextKeyStore } from '../../../host/ContextKeyService'
import { useConfigStore, useConfigValue } from '../../../stores/configStore'
import type { ConfigSection, ConfigSchema } from '../../../types/plugin'
import type { CommunityPluginManifest } from '../../../host/communityPluginLoader'

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

type NavTab = 'settings' | 'plugins'

export function SettingsPanelView() {
  const visible    = useContextKey('settingsPanelVisible') as boolean
  const sections   = useConfigSections()
  const plugins    = usePluginList()
  const community  = useCommunityManifests()

  const [navTab,        setNavTab]        = useState<NavTab>('settings')
  const [query,         setQuery]         = useState('')
  const [activeSection, setActiveSection] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)

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
        {navTab === 'settings' ? (
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
        ) : (
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
              <code>~/.tauri-shell/plugins/</code> then restart.
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
