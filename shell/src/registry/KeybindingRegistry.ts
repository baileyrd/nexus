// src/registry/KeybindingRegistry.ts
// Maps keyboard chords to command IDs, with optional 'when' context conditions.
//
// User overrides (WI-04 / Phase 2 §3.4):
//   - Each registered binding has a `defaultChord` baked in from the
//     manifest contribution. The active chord (`chord`) defaults to the
//     same value but is replaced with a user-set override at runtime.
//   - Overrides live in a small Map<commandId, chord> hydrated from a
//     pluggable `OverrideStorage` (see below) and persisted on every
//     mutation. The storage abstraction means the registry never has
//     to import @tauri-apps or knowledge of localStorage / kernel IPC
//     — the settings plugin chooses the backend at wiring time.
//   - Overrides are matched against `commandId`, not chord, so adding
//     or clearing one always applies to *every* binding for that
//     command (covers the case where multiple plugins contribute the
//     same command ID; rare in practice but well defined).

import type { KeybindingContribution } from '../types/plugin'
import { evaluateWhen } from '../host/ContextKeyService'

interface KeybindingEntry {
  id: string
  pluginId: string
  /** The currently active chord (override if present, otherwise default). */
  chord: string
  /** The chord declared in the plugin manifest, before overrides. */
  defaultChord: string
  commandId: string
  when?: string
}

/**
 * Pluggable persistence interface for keybinding overrides. Returns and
 * accepts a `Record<commandId, chord>` blob — the registry treats it as
 * an opaque JSON document. Implementations live next to the consumer
 * (settings plugin uses `api.storage`, tests use an in-memory shim).
 */
export interface OverrideStorage {
  read(): Promise<Record<string, string>>
  write(overrides: Record<string, string>): Promise<void>
}

export interface BindingRow {
  commandId: string
  /** Active chord (override or default). */
  current: string
  /** The manifest default. */
  default: string
  /** True iff `current !== default` *and* an override is present. */
  overridden: boolean
}

export class KeybindingRegistry {
  private bindings: KeybindingEntry[] = []
  private overrides = new Map<string, string>()

  registerFromManifest(pluginId: string, contribution: KeybindingContribution) {
    const isMac = typeof navigator !== 'undefined' &&
      navigator.platform.toLowerCase().includes('mac')
    const rawChord = (isMac && contribution.mac) ? contribution.mac : contribution.key
    const defaultChord = normalizeChord(rawChord)
    const override = this.overrides.get(contribution.command)

    this.bindings.push({
      id: `${pluginId}:${contribution.command}`,
      pluginId,
      chord: override ?? defaultChord,
      defaultChord,
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

  /**
   * Return the first binding registered for `commandId`, or `undefined`
   * when no plugin has declared one. The returned `chord` is the active
   * binding (user override if present, otherwise manifest default), so
   * callers rendering a hint — e.g. the editor's empty-state shortcut
   * pills — can pipe it straight through `formatChord` without a
   * second override lookup.
   *
   * When multiple plugins contribute the same commandId (rare but well
   * defined — see the override-matching note at the top of this file),
   * the first registered wins. Callers that need every contributor
   * should use `getAllBindings` / `all` instead.
   */
  findByCommand(commandId: string): KeybindingEntry | undefined {
    return this.bindings.find((b) => b.commandId === commandId)
  }

  /**
   * Shortcut for UI labels — resolve the active chord for `commandId`
   * and return it in the display shape `formatChord` produces (e.g.
   * `"Ctrl+N"`). Returns `undefined` when no binding matches so
   * callers can fall back to a documented default.
   *
   * Exposed as a method so UI consumers that already hold the
   * registry (via `getRegistry()`) don't need to import `formatChord`
   * from `shell/src/registry/*` — that path is gated by the plugin
   * import-hygiene guardrail (WI-23). Keeping the formatter adjacent
   * to the registry also means a future chord-style change lands in
   * one place.
   */
  formattedChordFor(commandId: string): string | undefined {
    const hit = this.findByCommand(commandId)
    return hit ? formatChord(hit.chord) : undefined
  }

  // ─── Overrides API (WI-04) ─────────────────────────────────────────────────

  /**
   * Hydrate the override map from `storage` and re-apply each entry to
   * any already-registered bindings. Idempotent — calling twice with the
   * same backing store leaves the registry in the same shape.
   */
  async loadOverrides(storage: OverrideStorage): Promise<void> {
    let loaded: Record<string, string> = {}
    try {
      loaded = await storage.read()
    } catch (err) {
      console.warn('[KeybindingRegistry] loadOverrides failed:', err)
      return
    }
    this.overrides = new Map(Object.entries(loaded))
    this.applyOverridesToBindings()
  }

  /**
   * Set or replace the override for `commandId` and persist. The new
   * chord is normalised before storage so on-disk shapes are stable
   * regardless of how the UI captured them.
   */
  async setOverride(
    storage: OverrideStorage,
    commandId: string,
    chord: string,
  ): Promise<void> {
    const normalized = normalizeChord(chord)
    this.overrides.set(commandId, normalized)
    this.applyOverridesToBindings()
    await this.persist(storage)
  }

  /** Clear an override and revert affected bindings to their default chord. */
  async clearOverride(storage: OverrideStorage, commandId: string): Promise<void> {
    if (!this.overrides.delete(commandId)) return
    this.applyOverridesToBindings()
    await this.persist(storage)
  }

  /** Synchronous read of the in-memory override map for a single command. */
  getOverride(commandId: string): string | undefined {
    return this.overrides.get(commandId)
  }

  /** All known commandId → chord overrides. Mostly useful for diagnostics. */
  getOverrides(): Record<string, string> {
    return Object.fromEntries(this.overrides)
  }

  /**
   * One row per *commandId* (deduped across plugins). Used by the
   * settings UI to render the keybindings table. Commands with no
   * binding contribution are not included; the settings UI can layer
   * those in from the CommandRegistry separately if needed.
   */
  getAllBindings(): BindingRow[] {
    const seen = new Set<string>()
    const out: BindingRow[] = []
    for (const b of this.bindings) {
      if (seen.has(b.commandId)) continue
      seen.add(b.commandId)
      const override = this.overrides.get(b.commandId)
      out.push({
        commandId: b.commandId,
        current: override ?? b.defaultChord,
        default: b.defaultChord,
        overridden: override !== undefined && override !== b.defaultChord,
      })
    }
    return out
  }

  // ─── Internal helpers ──────────────────────────────────────────────────────

  /** Re-apply the override map to every registered binding's `chord` field. */
  private applyOverridesToBindings() {
    for (const b of this.bindings) {
      const override = this.overrides.get(b.commandId)
      b.chord = override ?? b.defaultChord
    }
  }

  private async persist(storage: OverrideStorage): Promise<void> {
    try {
      await storage.write(Object.fromEntries(this.overrides))
    } catch (err) {
      console.error('[KeybindingRegistry] persist failed:', err)
      throw err
    }
  }
}

// ─── Utilities ────────────────────────────────────────────────────────────────

/**
 * Canonicalise chord strings. `Cmd+Shift+K`, `meta+shift+k`,
 * `Shift+Cmd+K` all collapse to the same `meta+shift+k`. We use
 * `meta` as the canonical name for the Cmd / Super / Win key.
 */
export function normalizeChord(chord: string): string {
  const aliases: Record<string, string> = {
    cmd: 'meta',
    command: 'meta',
    win: 'meta',
    super: 'meta',
    option: 'alt',
    opt: 'alt',
    control: 'ctrl',
    return: 'enter',
    esc: 'escape',
  }
  return chord
    .toLowerCase()
    .split('+')
    .map(k => k.trim())
    .map(k => aliases[k] ?? k)
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

/**
 * Pretty-print a normalised chord for display in the settings UI.
 * `meta+shift+k` → `Meta+Shift+K`. The settings table shows this form;
 * persistence stores the lowercase canonical form.
 */
export function formatChord(chord: string): string {
  if (!chord) return ''
  return chord
    .split('+')
    .map(part =>
      part.length === 1
        ? part.toUpperCase()
        : part.charAt(0).toUpperCase() + part.slice(1),
    )
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

// When-clause evaluation is delegated to `evaluateWhen` in
// `../host/ContextKeyService` (see the import at the top of this file).
// That implementation is a hand-rolled recursive-descent parser that
// evaluates the same grammar (&&, ||, !, ==, !=, parens, string/boolean
// literals, context-key lookups) without `new Function()` / `eval`, so
// the host webview can ship with CSP `script-src 'self'` (no
// `'unsafe-eval'`). WI-30a §4.2 + §10 Q1.
