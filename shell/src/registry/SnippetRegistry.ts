// src/registry/SnippetRegistry.ts
// OI-18 — Snippet trigger collision detection.
//
// Stores snippets contributed by plugins and emits
// `plugins:snippets-conflict` whenever the set of trigger-collisions
// changes. Mirrors the KeybindingRegistry conflict-detection pattern
// (keyed by trigger string instead of chord).
//
// The registry is intentionally data-only: it stores the snippet
// metadata and detects collisions. Actual expansion in the editor
// is a separate concern (tracked as a future work item).

import type { Snippet } from '@nexus/extension-api'
import type { SnippetContribution } from '../types/plugin'
import { eventBus } from '../host/EventBus'

// ─── Public shapes ────────────────────────────────────────────────────────────

export type { SnippetContribution }

export interface SnippetEntry {
  id:          string
  trigger:     string
  body:        string
  description?: string
  fileTypes?:  string[]
  pluginId:    string
}

export interface SnippetConflict {
  /** The trigger string shared by two or more snippets. */
  trigger: string
  /** Every snippet that claims this trigger. */
  entries: Array<{ pluginId: string; snippetId: string }>
}

// ─── Registry ─────────────────────────────────────────────────────────────────

export class SnippetRegistry {
  private snippets = new Map<string, SnippetEntry>()
  private lastConflictSig = '[]'

  /**
   * Called by ExtensionHost Pass 1 for manifest-declared snippets.
   * Idempotent: a second call for the same id is a no-op so the
   * eager-activation path doesn't double-register.
   */
  registerFromManifest(pluginId: string, contribution: SnippetContribution) {
    if (!this.snippets.has(contribution.id)) {
      this.snippets.set(contribution.id, { ...contribution, pluginId })
    }
    this.maybeEmitConflicts()
  }

  /**
   * Called from `api.editor.registerSnippet` during activate().
   * Overwrites an existing entry with the same id (the runtime body
   * may differ from the manifest-declared one).
   */
  register(pluginId: string, snippet: Snippet) {
    this.snippets.set(snippet.id, { ...snippet, pluginId })
    this.maybeEmitConflicts()
  }

  unregister(id: string) {
    this.snippets.delete(id)
    this.maybeEmitConflicts()
  }

  all(): SnippetEntry[] {
    return [...this.snippets.values()]
  }

  /**
   * Returns every trigger string claimed by two or more snippets.
   * `fileTypes` scoping is intentionally ignored for the MVP — any
   * two snippets with the same trigger are a potential conflict
   * regardless of file type.
   */
  getConflicts(): SnippetConflict[] {
    const byTrigger = new Map<string, SnippetEntry[]>()
    for (const entry of this.snippets.values()) {
      const list = byTrigger.get(entry.trigger)
      if (list) list.push(entry)
      else byTrigger.set(entry.trigger, [entry])
    }
    const conflicts: SnippetConflict[] = []
    for (const [trigger, entries] of byTrigger) {
      if (entries.length < 2) continue
      conflicts.push({
        trigger,
        entries: entries.map(e => ({ pluginId: e.pluginId, snippetId: e.id })),
      })
    }
    return conflicts
  }

  private maybeEmitConflicts() {
    const conflicts = this.getConflicts()
    const sig = JSON.stringify(conflicts)
    if (sig === this.lastConflictSig) return
    this.lastConflictSig = sig
    eventBus.emit('plugins:snippets-conflict', { conflicts })
  }
}
