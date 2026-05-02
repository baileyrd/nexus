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
import { eventBus } from '../host/EventBus'
import { clientLogger } from '../host/clientLogger'

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
 * A chord whose active binding is registered against more than one
 * commandId in a way `match()` would treat as ambiguous (OI-10).
 *
 * Two bindings conflict when they share a normalised chord *and* their
 * `when` clauses overlap. We don't have a real boolean-formula solver,
 * so the conservative rule we apply is:
 *
 *   1. exactly equal `when` strings (incl. both `undefined`) → conflict
 *   2. at least one side has no `when` (matches everywhere)        → conflict
 *   3. both sides have differing `when` strings                    → not flagged
 *
 * Case 3 might be a real conflict (e.g. `editorFocus` vs `!terminalFocus`
 * can overlap) but flagging it without analysis would generate too many
 * false positives. Plugin authors who rely on disjoint contexts get the
 * benefit of the doubt; users who hit a real ambiguity in case 3 will
 * still see it as a "shortcut doesn't work" bug rather than a phantom
 * collision warning.
 */
export interface KeybindingConflict {
  /** The normalised chord that more than one binding is bound to. */
  chord: string
  /** Every entry sharing the chord — usually 2, occasionally more. */
  entries: Array<{
    pluginId: string
    commandId: string
    when?: string
  }>
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
  /**
   * Other commandIds whose active chord matches `current` and whose
   * `when` clause overlaps this row's. Empty when there's no conflict.
   * Populated by `getAllBindings()` so the UI can render a warning
   * without a second registry call.
   */
  conflictsWith: string[]
}

export class KeybindingRegistry {
  private bindings: KeybindingEntry[] = []
  private overrides = new Map<string, string>()
  private storage: OverrideStorage | null = null
  /**
   * JSON of the last conflict set we emitted. Lets bulk manifest
   * registration converge to a single `plugins:keybindings-conflict`
   * event at boot — only mutations that actually change the conflict
   * set re-emit.
   */
  private lastConflictSignature: string = '[]'

  /**
   * Bind a storage backend for override persistence. Must be called
   * before `loadOverrides()` / `setOverride()` / `clearOverride()`.
   * Calling twice is a no-op with a console warning (double-wire guard).
   */
  bindStorage(storage: OverrideStorage): void {
    if (this.storage !== null) {
      clientLogger.warn('[KeybindingRegistry] bindStorage called more than once — ignoring')
      return
    }
    this.storage = storage
  }

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
    this.maybeEmitConflicts()
  }

  unregister(id: string) {
    this.bindings = this.bindings.filter(b => b.id !== id)
    this.maybeEmitConflicts()
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
   * Hydrate the override map from the bound storage and re-apply each
   * entry to any already-registered bindings. Idempotent — calling
   * twice with the same backing store leaves the registry in the same
   * shape. No-ops (with a warning) if no storage has been bound.
   */
  async loadOverrides(): Promise<void> {
    if (!this.storage) {
      clientLogger.warn('[KeybindingRegistry] loadOverrides called before bindStorage — no-op')
      return
    }
    let loaded: Record<string, string> = {}
    try {
      loaded = await this.storage.read()
    } catch (err) {
      clientLogger.warn('[KeybindingRegistry] loadOverrides failed:', err)
      return
    }
    this.overrides = new Map(Object.entries(loaded))
    this.applyOverridesToBindings()
    this.maybeEmitConflicts()
  }

  /**
   * Set or replace the override for `commandId` and persist. The new
   * chord is normalised before storage so on-disk shapes are stable
   * regardless of how the UI captured them. No-ops (with a warning) if
   * no storage has been bound.
   */
  async setOverride(
    commandId: string,
    chord: string,
  ): Promise<void> {
    if (!this.storage) {
      clientLogger.warn('[KeybindingRegistry] setOverride called before bindStorage — no-op')
      return
    }
    const normalized = normalizeChord(chord)
    this.overrides.set(commandId, normalized)
    this.applyOverridesToBindings()
    this.maybeEmitConflicts()
    await this.persist()
  }

  /** Clear an override and revert affected bindings to their default chord.
   *  No-ops (with a warning) if no storage has been bound. */
  async clearOverride(commandId: string): Promise<void> {
    if (!this.storage) {
      clientLogger.warn('[KeybindingRegistry] clearOverride called before bindStorage — no-op')
      return
    }
    if (!this.overrides.delete(commandId)) return
    this.applyOverridesToBindings()
    this.maybeEmitConflicts()
    await this.persist()
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
   *
   * Each row's `conflictsWith` is populated from the same conflict
   * computation that drives the `plugins:keybindings-conflict` event,
   * so the UI never has to call `getConflicts()` separately.
   */
  getAllBindings(): BindingRow[] {
    const conflictsByCommandId = new Map<string, string[]>()
    for (const c of this.computeConflicts()) {
      const ids = c.entries.map(e => e.commandId)
      for (const id of ids) {
        const others = ids.filter(o => o !== id)
        const acc = conflictsByCommandId.get(id) ?? []
        for (const o of others) if (!acc.includes(o)) acc.push(o)
        conflictsByCommandId.set(id, acc)
      }
    }
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
        conflictsWith: conflictsByCommandId.get(b.commandId) ?? [],
      })
    }
    return out
  }

  /**
   * All currently-detected chord conflicts, one entry per chord with
   * the full set of competing bindings. Returns an empty array when
   * nothing collides. See {@link KeybindingConflict} for the conflict
   * definition.
   */
  getConflicts(): KeybindingConflict[] {
    return this.computeConflicts()
  }

  // ─── Internal helpers ──────────────────────────────────────────────────────

  /**
   * Walk the binding list and group anything that shares an active
   * chord *and* an overlapping `when` clause. The overlap rule is the
   * conservative one documented on {@link KeybindingConflict} — we'd
   * rather miss a real conflict (case 3, both sides have differing
   * `when`s) than spam the UI with phantom collisions.
   */
  private computeConflicts(): KeybindingConflict[] {
    // Bucket bindings by active chord. A conflict needs at least two
    // entries in the same bucket, so chords with one binding short-
    // circuit before we look at `when` clauses at all.
    const byChord = new Map<string, KeybindingEntry[]>()
    for (const b of this.bindings) {
      const list = byChord.get(b.chord)
      if (list) list.push(b)
      else byChord.set(b.chord, [b])
    }

    const conflicts: KeybindingConflict[] = []
    for (const [chord, group] of byChord) {
      if (group.length < 2) continue

      // Walk the bucket once and collect every entry that overlaps any
      // earlier entry. Using a Set keyed by `id` so the same entry isn't
      // pushed twice when it overlaps multiple earlier ones.
      const colliding = new Map<string, KeybindingEntry>()
      for (let i = 0; i < group.length; i++) {
        for (let j = i + 1; j < group.length; j++) {
          if (whenOverlaps(group[i].when, group[j].when)) {
            colliding.set(group[i].id, group[i])
            colliding.set(group[j].id, group[j])
          }
        }
      }
      if (colliding.size === 0) continue

      conflicts.push({
        chord,
        entries: [...colliding.values()].map(e => ({
          pluginId: e.pluginId,
          commandId: e.commandId,
          when: e.when,
        })),
      })
    }
    // Stable order — alphabetical by chord — so the signature dedup and
    // the UI both see a deterministic shape.
    conflicts.sort((a, b) => a.chord.localeCompare(b.chord))
    return conflicts
  }

  /**
   * Recompute conflicts and emit `plugins:keybindings-conflict` if the
   * set changed since the last emission. Bulk manifest registration at
   * boot therefore converges to a single event (or zero, when no
   * conflicts exist) — we only fire when listeners would actually see
   * a different payload.
   */
  private maybeEmitConflicts(): void {
    const conflicts = this.computeConflicts()
    const sig = JSON.stringify(conflicts)
    if (sig === this.lastConflictSignature) return
    this.lastConflictSignature = sig
    eventBus.emit('plugins:keybindings-conflict', { conflicts })
  }

  /** Re-apply the override map to every registered binding's `chord` field. */
  private applyOverridesToBindings() {
    for (const b of this.bindings) {
      const override = this.overrides.get(b.commandId)
      b.chord = override ?? b.defaultChord
    }
  }

  private async persist(): Promise<void> {
    try {
      await this.storage!.write(Object.fromEntries(this.overrides))
    } catch (err) {
      clientLogger.error('[KeybindingRegistry] persist failed:', err)
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

/**
 * Conservative `when`-clause overlap test used for conflict detection.
 * Returns `true` when two clauses are guaranteed to overlap (and so a
 * shared chord is a real ambiguity), `false` when they *might* be
 * disjoint. See {@link KeybindingConflict} for the full table.
 */
function whenOverlaps(a: string | undefined, b: string | undefined): boolean {
  // Both unconditional, or both gated by the same expression — always
  // overlap.
  if (a === b) return true
  // Exactly one side is unconditional — that side matches in every
  // context, so any shared chord with a gated sibling is a conflict.
  if (a === undefined || b === undefined) return true
  // Both gated by differing expressions: we don't have a solver and
  // would rather miss a real conflict than over-warn.
  return false
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
