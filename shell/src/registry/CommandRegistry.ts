// src/registry/CommandRegistry.ts
// Central catalog of all registered commands.
// Manifest contributions populate labels; activate() wires handlers.

import type { CommandContribution, CommandEntry } from '../types/plugin'
import { activationTriggers } from '../host/ActivationTriggers'
import { eventBus } from '../host/EventBus'

export class CommandRegistry {
  private commands = new Map<string, CommandEntry & { handler?: (...args: unknown[]) => unknown }>()

  /** Called by the extension host before activate() — populates label/metadata */
  registerFromManifest(pluginId: string, contribution: CommandContribution) {
    if (!this.commands.has(contribution.id)) {
      this.commands.set(contribution.id, {
        id: contribution.id,
        title: contribution.title,
        category: contribution.category,
        pluginId,
        handler: undefined,
      })
    }
  }

  /** Called from activate() — wires the handler to an existing or new entry */
  register(pluginId: string, id: string, handler: (...args: unknown[]) => unknown) {
    const existing = this.commands.get(id)
    if (existing) {
      existing.handler = handler
    } else {
      this.commands.set(id, { id, title: id, pluginId, handler })
    }
  }

  unregister(id: string) {
    this.commands.delete(id)
  }

  async execute(id: string, ...args: unknown[]): Promise<unknown> {
    // WI-19 — wake any plugin gated on `onCommand:<id>` *before* the
    // lookup. The trigger fire resolves once activation finishes, so a
    // freshly-woken plugin's `register()` call has already populated the
    // map by the time we read it back below. No-op when nothing is gated
    // (the `hasPending` short-circuit avoids the await on the hot path).
    const triggerKey = `onCommand:${id}`
    if (activationTriggers.hasPending(triggerKey)) {
      await activationTriggers.fire(triggerKey)
    }
    const cmd = this.commands.get(id)
    if (!cmd?.handler) {
      console.warn(`[CommandRegistry] No handler for command '${id}'`)
      return
    }
    // WI-35 — per-plugin crash quarantine (Q3 default: re-throw).
    // A handler that panics must not leave the registry in an
    // inconsistent state: the map entry stays, sibling commands stay
    // callable, and the error is surfaced on the event bus as
    // `command:error` so UI layers (notification service, status bar)
    // can react without the caller having to catch. We still re-throw
    // so the awaiter — typically the command palette / keybinding
    // dispatcher — can decide whether to show a modal, retry, etc.
    // (Unlike EventBus.emit, which swallows per-listener failures
    // because event dispatch has no single point to catch.)
    try {
      return await cmd.handler(...args)
    } catch (err) {
      console.error(`[CommandRegistry] Command '${id}' threw:`, err)
      try {
        eventBus.emit('command:error', {
          commandId: id,
          pluginId: cmd.pluginId,
          error: err instanceof Error ? err.message : String(err),
        })
      } catch {
        // eventBus.emit already swallows per-listener errors; the
        // outer catch is belt-and-braces for the extraordinarily
        // unlikely case that the map lookup itself throws.
      }
      throw err
    }
  }

  all(): CommandEntry[] {
    return [...this.commands.values()].map(({ handler: _h, ...entry }) => entry)
  }

  get(id: string): (CommandEntry & { handler?: (...args: unknown[]) => unknown }) | undefined {
    return this.commands.get(id)
  }

  has(id: string): boolean {
    return this.commands.has(id)
  }
}
