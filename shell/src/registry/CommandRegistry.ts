// src/registry/CommandRegistry.ts
// Central catalog of all registered commands.
// Manifest contributions populate labels; activate() wires handlers.

import type { CommandContribution, CommandEntry } from '../types/plugin'
import { activationTriggers } from '../host/ActivationTriggers'
import { eventBus } from '../host/EventBus'
import { configStore } from '../stores/configStore'

/** OI-11 — defaults, also re-used as the test override floor. */
const DEFAULT_WARN_MS = 250
const DEFAULT_CANCEL_MS = 5000

/**
 * Thrown by `CommandRegistry.execute` when a handler hasn't resolved
 * within `shell.command.timeoutCancelMs`. The handler keeps running —
 * JavaScript promises aren't cancellable — but the awaiter gets
 * control back so the UI can move on (palette dismiss, status-bar
 * spinner clear, etc.). Distinct from a regular error via `name`.
 */
export class CommandCancelledError extends Error {
  readonly name = 'CommandCancelled'
  constructor(
    readonly commandId: string,
    readonly thresholdMs: number,
  ) {
    super(`Command '${commandId}' did not resolve within ${thresholdMs}ms`)
  }
}

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
    //
    // OI-11 — UI-thread time budget. We race the handler against a
    // configurable cancel deadline (`shell.command.timeoutCancelMs`,
    // default 5s) and log a soft warning at the warn threshold
    // (`shell.command.timeoutWarnMs`, default 250ms). Either timeout
    // set to 0 or below disables that tier — useful for tests and for
    // users who explicitly opt out of cancellation. The handler keeps
    // running after a hard cancel; this only releases the awaiter.
    const warnMs = configStore.get('shell.command.timeoutWarnMs', DEFAULT_WARN_MS)
    const cancelMs = configStore.get('shell.command.timeoutCancelMs', DEFAULT_CANCEL_MS)
    let warnTimer: ReturnType<typeof setTimeout> | undefined
    let cancelTimer: ReturnType<typeof setTimeout> | undefined

    if (warnMs > 0) {
      warnTimer = setTimeout(() => {
        console.warn(
          `[CommandRegistry] Command '${id}' (plugin '${cmd.pluginId}') still pending after ${warnMs}ms`,
        )
      }, warnMs)
    }

    const cancelPromise = cancelMs > 0
      ? new Promise<never>((_, reject) => {
          cancelTimer = setTimeout(() => {
            const err = new CommandCancelledError(id, cancelMs)
            try {
              eventBus.emit('command:cancelled', {
                commandId: id,
                pluginId: cmd.pluginId,
                thresholdMs: cancelMs,
              })
            } catch {
              // Belt-and-braces — see the matching note in the error
              // path below.
            }
            reject(err)
          }, cancelMs)
        })
      : null
    // If the handler resolves first, the cancel promise stays pending
    // forever and gets GC'd with its captured closure once `execute`
    // returns. Attach a sink so any future rejection (which only
    // happens if our finally-clear lost a race — it can't, because
    // microtasks drain before macrotasks, but belt-and-braces) does
    // not surface as an unhandled rejection.
    cancelPromise?.catch(() => {})

    const handlerPromise = (async () => cmd.handler!(...args))()

    try {
      return await (cancelPromise
        ? Promise.race([handlerPromise, cancelPromise])
        : handlerPromise)
    } catch (err) {
      if (err instanceof CommandCancelledError) {
        console.warn(`[CommandRegistry] Command '${id}' hard-cancelled after ${cancelMs}ms`)
        // The handler is still running — silently swallow whatever it
        // produces so an unhandled-rejection handler doesn't fire
        // *and* the original cancel still surfaces to the caller.
        handlerPromise.catch(() => {})
        throw err
      }
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
    } finally {
      if (warnTimer !== undefined) clearTimeout(warnTimer)
      if (cancelTimer !== undefined) clearTimeout(cancelTimer)
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
