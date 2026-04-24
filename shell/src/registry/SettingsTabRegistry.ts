// src/registry/SettingsTabRegistry.ts
//
// Plugin-contributed tabs in the Settings modal (OI-01).
//
// Follows the CommandRegistry/KeybindingRegistry two-phase pattern:
//
//   Phase 1 — `registerFromManifest(pluginId, contribution)` records the
//     tab's metadata (title, icon, group, priority) so the settings
//     panel can show the entry in the left rail even if the plugin
//     hasn't activated yet. Renderer is `undefined` at this point.
//
//   Phase 2 — `register(pluginId, id, renderer)` is called from the
//     plugin's `activate()` via `api.settings.registerTab(id, Comp)`.
//     It wires the React component that draws the right-pane content.
//
// Tabs with no renderer attached (manifest-declared but the plugin
// hasn't activated or never calls `registerTab`) are filtered out of
// `all()` so the UI doesn't show an empty rail entry. Activation of a
// plugin gated on `onView:settingsTab:<id>` is a follow-up; for now
// plugins should register settings tabs from `onStartup` or another
// eagerly-fired trigger.

import type { ComponentType } from 'react'
import type { SettingsTabContribution, SettingsTabEntry } from '../types/plugin'

export type SettingsTabRenderer = ComponentType<Record<string, never>>

type StoredEntry = SettingsTabEntry & { renderer?: SettingsTabRenderer }

export class SettingsTabRegistry {
  private tabs = new Map<string, StoredEntry>()

  /** Called by ExtensionHost before `activate()` — populates metadata only. */
  registerFromManifest(pluginId: string, contribution: SettingsTabContribution) {
    if (this.tabs.has(contribution.id)) return
    this.tabs.set(contribution.id, {
      id: contribution.id,
      title: contribution.title,
      icon: contribution.icon,
      group: contribution.group,
      priority: contribution.priority ?? 50,
      pluginId,
      renderer: undefined,
    })
  }

  /** Called from `activate()` — wires the renderer to an existing or new entry. */
  register(pluginId: string, id: string, renderer: SettingsTabRenderer, meta?: Partial<SettingsTabContribution>) {
    const existing = this.tabs.get(id)
    if (existing) {
      existing.renderer = renderer
      if (meta?.title) existing.title = meta.title
      if (meta?.icon) existing.icon = meta.icon
      if (meta?.group) existing.group = meta.group
      if (typeof meta?.priority === 'number') existing.priority = meta.priority
    } else {
      this.tabs.set(id, {
        id,
        title: meta?.title ?? id,
        icon: meta?.icon,
        group: meta?.group,
        priority: meta?.priority ?? 50,
        pluginId,
        renderer,
      })
    }
  }

  unregister(id: string) {
    this.tabs.delete(id)
  }

  /** All tabs with a wired renderer, stable-sorted by (group, priority, id). */
  all(): SettingsTabEntry[] {
    const order: Record<string, number> = {
      options: 0,
      'core-plugins': 1,
      'community-plugins': 2,
    }
    return [...this.tabs.values()]
      .filter((t): t is StoredEntry & { renderer: SettingsTabRenderer } => Boolean(t.renderer))
      .sort((a, b) => {
        const ga = order[a.group ?? 'options'] ?? 99
        const gb = order[b.group ?? 'options'] ?? 99
        if (ga !== gb) return ga - gb
        const pa = a.priority ?? 50
        const pb = b.priority ?? 50
        if (pa !== pb) return pa - pb
        return a.id.localeCompare(b.id)
      })
      .map(({ renderer: _r, ...entry }) => entry)
  }

  /** Renderer lookup for the active tab. */
  getRenderer(id: string): SettingsTabRenderer | undefined {
    return this.tabs.get(id)?.renderer
  }

  has(id: string): boolean {
    return this.tabs.has(id)
  }
}
