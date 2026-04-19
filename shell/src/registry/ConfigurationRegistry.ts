// src/registry/ConfigurationRegistry.ts
// Stores plugin-declared configuration schemas.
// The settings panel UI plugin reads from this to auto-generate settings UI.

import type { ConfigSection } from '../types/plugin'

export class ConfigurationRegistry {
  private sections = new Map<string, ConfigSection>()

  register(section: ConfigSection) {
    this.sections.set(section.pluginId, section)
  }

  unregister(pluginId: string) {
    this.sections.delete(pluginId)
  }

  all(): ConfigSection[] {
    return [...this.sections.values()]
      .sort((a, b) => a.order - b.order)
  }

  get(pluginId: string): ConfigSection | undefined {
    return this.sections.get(pluginId)
  }
}
