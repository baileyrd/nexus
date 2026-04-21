// src/registry/KeybindingRegistry.ts
// Maps keyboard chords to command IDs, with optional 'when' context conditions.

import type { KeybindingContribution } from '../types/plugin'

interface KeybindingEntry {
  id: string
  pluginId: string
  chord: string
  commandId: string
  when?: string
}

export class KeybindingRegistry {
  private bindings: KeybindingEntry[] = []

  registerFromManifest(pluginId: string, contribution: KeybindingContribution) {
    const isMac = typeof navigator !== 'undefined' &&
      navigator.platform.toLowerCase().includes('mac')
    const rawChord = (isMac && contribution.mac) ? contribution.mac : contribution.key

    this.bindings.push({
      id: `${pluginId}:${contribution.command}`,
      pluginId,
      chord: normalizeChord(rawChord),
      commandId: contribution.command,
      when: contribution.when,
    })
  }

  unregister(id: string) {
    this.bindings = this.bindings.filter(b => b.id !== id)
  }

  /**
   * Find the matching command ID for a keyboard event.
   * Returns null if no match or 'when' condition is false.
   */
  match(
    event: KeyboardEvent,
    contextKeys: Record<string, unknown>
  ): string | null {
    const chord = eventToChord(event)

    for (const binding of this.bindings) {
      if (binding.chord !== chord) continue
      if (binding.when && !evaluateWhen(binding.when, contextKeys)) continue
      return binding.commandId
    }

    return null
  }

  all(): KeybindingEntry[] {
    return [...this.bindings]
  }
}

// ─── Utilities ────────────────────────────────────────────────────────────────

function normalizeChord(chord: string): string {
  return chord
    .toLowerCase()
    .split('+')
    .map(k => k.trim())
    .sort((a, b) => {
      // canonical order: ctrl, shift, alt, meta, then key
      const order = ['ctrl', 'shift', 'alt', 'meta']
      const ai = order.indexOf(a)
      const bi = order.indexOf(b)
      if (ai >= 0 && bi >= 0) return ai - bi
      if (ai >= 0) return -1
      if (bi >= 0) return 1
      return 0
    })
    .join('+')
}

function eventToChord(event: KeyboardEvent): string {
  const parts: string[] = []
  if (event.ctrlKey)  parts.push('ctrl')
  if (event.shiftKey) parts.push('shift')
  if (event.altKey)   parts.push('alt')
  if (event.metaKey)  parts.push('meta')

  const key = event.key.toLowerCase()
  if (!['control', 'shift', 'alt', 'meta'].includes(key)) {
    parts.push(key)
  }

  return parts.join('+')
}

/**
 * Simple when-clause evaluator.
 * Supports: &&, ||, !, ==, !=, bare key names, parentheses.
 */
function evaluateWhen(expression: string, keys: Record<string, unknown>): boolean {
  if (!expression?.trim()) return true

  try {
    // Replace key references with their values
    // This is a simple approach — for production, use a proper expression parser
    const normalized = expression
      .replace(/(\w+(?:\.\w+)*)/g, (match) => {
        // Skip operators
        if (['true', 'false', 'null', 'undefined'].includes(match)) return match
        const val = keys[match]
        if (val === undefined) return 'false'
        if (typeof val === 'boolean') return val.toString()
        if (typeof val === 'string') return JSON.stringify(val)
        return String(val)
      })

    // Evaluate — note: in production replace with a proper safe evaluator
    // eslint-disable-next-line no-new-func
    return Boolean(new Function(`return (${normalized})`)())
  } catch {
    return false
  }
}
