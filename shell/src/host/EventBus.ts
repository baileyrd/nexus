// src/host/EventBus.ts
// Typed publish/subscribe for decoupled plugin communication.
// Plugins never import each other directly — they use events.

import { clientLogger } from './clientLogger'

type Handler<T = unknown> = (payload: T) => void

class EventBus {
  private handlers = new Map<string, Set<Handler>>()

  /**
   * Subscribe to an event.
   * Returns an unsubscribe function — call it in deactivate().
   */
  on<T = unknown>(event: string, handler: Handler<T>): () => void {
    if (!this.handlers.has(event)) {
      this.handlers.set(event, new Set())
    }
    this.handlers.get(event)!.add(handler as Handler)

    return () => {
      this.handlers.get(event)?.delete(handler as Handler)
    }
  }

  /**
   * Emit an event synchronously.
   * All handlers run before emit() returns.
   * Errors in individual handlers are caught and logged — they do not
   * prevent other handlers from running.
   */
  emit<T = unknown>(event: string, payload: T): void {
    // Specific handlers
    const handlers = this.handlers.get(event)
    if (handlers) {
      for (const handler of handlers) {
        try {
          handler(payload)
        } catch (err) {
          clientLogger.error(`[EventBus] Handler for '${event}' threw:`, err)
        }
      }
    }

    // Wildcard handlers receive every event
    const wildcardHandlers = this.handlers.get('*')
    if (wildcardHandlers) {
      const wrapped = { event, payload }
      for (const handler of wildcardHandlers) {
        try {
          handler(wrapped)
        } catch (err) {
          clientLogger.error(`[EventBus] Wildcard handler threw on '${event}':`, err)
        }
      }
    }
  }

  /**
   * Emit asynchronously — does not block the caller.
   * Use for high-frequency events (file watcher updates, editor keystrokes).
   */
  emitAsync<T = unknown>(event: string, payload: T): void {
    setTimeout(() => this.emit(event, payload), 0)
  }

  /** Subscribe to all events — useful for debugging and logging. */
  onAll(handler: (event: string, payload: unknown) => void): () => void {
    return this.on<{ event: string; payload: unknown }>('*', ({ event, payload }) => {
      handler(event, payload)
    })
  }

  /** Remove all handlers for a plugin (by unsubscribing all its returned fns). */
  clear(event?: string) {
    if (event) {
      this.handlers.delete(event)
    } else {
      this.handlers.clear()
    }
  }
}

// Singleton — shared across the entire shell and all plugins
export const eventBus = new EventBus()

// ─── Well-known event types ───────────────────────────────────────────────────

export interface ShellEvents {
  // Editor
  'editor:activeFileChanged': { path: string; content: string; language: string }
  'editor:contentChanged':    { path: string; content: string }
  'editor:fileSaved':         { path: string }

  // Filesystem
  'fs:fileCreated':           { path: string }
  'fs:fileDeleted':           { path: string }
  'fs:fileChanged':           { path: string }
  'fs:fileRenamed':           { oldPath: string; newPath: string }

  // Plugin lifecycle
  'plugin:activated':         { pluginId: string }
  'plugin:deactivated':       { pluginId: string }
  'plugin:error':             { pluginId: string; error: Error }

  // Keybinding conflicts (OI-10).
  // Emitted by KeybindingRegistry whenever the set of detected chord
  // conflicts changes — boot-time bulk registration converges to a
  // single event. Payload is the full current conflict set so the
  // settings UI can render or clear without a separate registry call.
  // `conflicts` is empty when the most recent mutation resolved every
  // outstanding conflict.
  'plugins:keybindings-conflict': {
    conflicts: Array<{
      chord: string
      entries: Array<{ pluginId: string; commandId: string; when?: string }>
    }>
  }

  // Command lifecycle (WI-35 — per-plugin crash quarantine).
  // CommandRegistry.execute emits this after a handler throws, just
  // before re-throwing to the caller. `pluginId` is the one that
  // registered the handler (may be undefined for manifest-only entries
  // that never got a handler wired).
  'command:error':            { commandId: string; pluginId?: string; error: string }

  // Command dispatch hard-cancel (OI-11).
  // Emitted by CommandRegistry.execute when a handler is still pending
  // after `shell.command.timeoutCancelMs` (default 5000ms). The caller
  // also receives a thrown `Error` with name `CommandCancelled` so
  // awaiters can distinguish cancellation from a regular failure.
  // The handler keeps executing in the background — JavaScript
  // promises aren't natively cancellable, so this is "stop awaiting,
  // restore UI responsiveness" rather than "kill the task".
  'command:cancelled':        { commandId: string; pluginId?: string; thresholdMs: number }

  // Shell
  'shell:ready':              Record<string, never>
  'shell:themeChanged':       { themeId: string }
  'shell:layoutChanged':      { layoutId: string }

  // Debug (example — contributed by a debug plugin)
  'debug:sessionStarted':     { sessionId: string }
  'debug:sessionEnded':       { sessionId: string }
}
