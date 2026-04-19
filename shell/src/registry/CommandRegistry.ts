// src/registry/CommandRegistry.ts
// Central catalog of all registered commands.
// Manifest contributions populate labels; activate() wires handlers.

import type { CommandContribution, CommandEntry } from '../types/plugin'

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
    const cmd = this.commands.get(id)
    if (!cmd?.handler) {
      console.warn(`[CommandRegistry] No handler for command '${id}'`)
      return
    }
    return cmd.handler(...args)
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
