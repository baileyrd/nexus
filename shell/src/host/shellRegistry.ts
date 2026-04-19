// src/host/shellRegistry.ts
// Singleton registry reference — avoids circular import between main.tsx and App.tsx.
// main.tsx sets this after boot; App.tsx reads it for the keybinding dispatcher.

import type { PluginRegistry } from './PluginRegistry'

let _registry: PluginRegistry | null = null

export function setRegistry(reg: PluginRegistry) {
  _registry = reg
}

export function getRegistry(): PluginRegistry | null {
  return _registry
}
