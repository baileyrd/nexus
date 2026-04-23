// src/host/ActivationTriggers.ts
//
// WI-19 — Process-wide registry of *deferred* plugin activation triggers.
//
// The `ExtensionHost` is the only writer: during `loadAll` Pass 1 it parses
// each manifest's `activationEvents` and calls `register('onView:foo',
// pluginId)`. Trigger sources (CommandRegistry, Leaf.setViewState,
// UriHandlerRegistry, future `onLanguage`) are the only readers: they call
// `fire('<trigger>')` immediately before doing the work the trigger gates,
// `await` the returned promise so the plugin's `activate()` finishes before
// the consumer continues, and then proceed normally.
//
// Layout mirrors the slot/uri-handler singletons: a class for testability +
// a process-wide instance the registries can import without taking a host
// reference. The host wires the activator on construction; until then, all
// `fire(...)` calls are no-ops (so unit tests that exercise registries in
// isolation don't need a host).
//
// Key shapes (string-encoded so the maps are flat and cheap to look up):
//   - 'onView:<viewId>'        — fired from Leaf.setViewState
//   - 'onCommand:<commandId>'  — fired from CommandRegistry.execute
//   - 'onUri:<scheme>'         — fired from UriHandlerRegistry.dispatch
//   - 'onLanguage:<lang>'      — fired from the editor open path (future)
//
// Once a plugin activates via a trigger, every key it owned is removed
// (subsequent fires of the same key become no-ops — the plugin is loaded).
// Failed activations also evict the keys so the trigger doesn't keep
// re-trying a known-broken plugin and stalling the dispatch path.

export type Activator = (pluginId: string) => Promise<void>

export class ActivationTriggers {
  // triggerKey ('onView:foo') → set of plugin ids waiting on it.
  // A plugin can list multiple triggers; the first one to fire wins
  // and evicts the plugin from every other key it owned.
  private byKey = new Map<string, Set<string>>()
  // Reverse index for eviction. `pluginId → Set<triggerKey>`.
  private byPlugin = new Map<string, Set<string>>()
  // Set when ExtensionHost constructs. Until then `fire` is a no-op so
  // registries instantiated from unit tests (without a host) don't blow up.
  private activator: Activator | null = null
  // Coalesces concurrent `fire` calls so two registries firing the same
  // trigger for the same plugin in the same tick don't both await two
  // distinct activate() invocations. The host's `activate()` is itself
  // idempotent on the second call (returns early when state === 'active'),
  // but coalescing here also lets the second caller observe the same
  // promise instead of racing the state machine.
  private inflight = new Map<string, Promise<void>>()

  setActivator(activator: Activator): void {
    this.activator = activator
  }

  /**
   * Register a deferred trigger. Called by `ExtensionHost.loadAll` Pass 1
   * for each (manifest, activationEvent) pair where the event is not
   * `onStartup`/`*`.
   */
  register(triggerKey: string, pluginId: string): void {
    if (!this.byKey.has(triggerKey)) {
      this.byKey.set(triggerKey, new Set())
    }
    this.byKey.get(triggerKey)!.add(pluginId)
    if (!this.byPlugin.has(pluginId)) {
      this.byPlugin.set(pluginId, new Set())
    }
    this.byPlugin.get(pluginId)!.add(triggerKey)
  }

  /**
   * Returns true iff at least one plugin is still waiting on this trigger.
   * Cheap pre-check for hot-path consumers (e.g. CommandRegistry.execute)
   * that want to skip the async hop when nothing is gated.
   */
  hasPending(triggerKey: string): boolean {
    const set = this.byKey.get(triggerKey)
    return !!(set && set.size > 0)
  }

  /**
   * Activate every plugin gated on `triggerKey`. Resolves once all
   * pending plugins have transitioned out of `activating` (success OR
   * failure). Failures are surfaced via the host's existing `plugin:error`
   * eventBus emission; this method itself never throws so a single bad
   * plugin can't break the trigger source's dispatch loop.
   */
  async fire(triggerKey: string): Promise<void> {
    if (!this.activator) return
    const pending = this.byKey.get(triggerKey)
    if (!pending || pending.size === 0) return

    // Snapshot + evict before activating: prevents re-entrant fires from
    // queueing the same plugin twice and matches "trigger consumed" semantics.
    const pluginIds = [...pending]
    for (const pid of pluginIds) {
      this.evict(pid)
    }

    const activator = this.activator
    const tasks = pluginIds.map((pid) => {
      const existing = this.inflight.get(pid)
      if (existing) return existing
      const task = activator(pid)
        .catch((err) => {
          // The host already logs + emits plugin:error; we don't rethrow
          // because the trigger source has nothing to do with the failure
          // (the user clicking a view button shouldn't see a thrown promise).
          console.warn(
            `[ActivationTriggers] activator for '${pid}' (trigger '${triggerKey}') threw:`,
            err,
          )
        })
        .finally(() => {
          this.inflight.delete(pid)
        })
      this.inflight.set(pid, task)
      return task
    })

    await Promise.all(tasks)
  }

  /**
   * Drop every trigger owned by `pluginId`. Called both internally on
   * activation (the plugin no longer needs to be lazily woken) and from
   * tests that want to reset state between cases.
   */
  evict(pluginId: string): void {
    const keys = this.byPlugin.get(pluginId)
    if (!keys) return
    for (const key of keys) {
      const set = this.byKey.get(key)
      if (set) {
        set.delete(pluginId)
        if (set.size === 0) this.byKey.delete(key)
      }
    }
    this.byPlugin.delete(pluginId)
  }

  /** Diagnostic — number of plugins still gated on a trigger. */
  pendingCount(): number {
    return this.byPlugin.size
  }

  /** Diagnostic — list of (triggerKey, pluginId) pairs still gated. */
  list(): Array<{ triggerKey: string; pluginId: string }> {
    const out: Array<{ triggerKey: string; pluginId: string }> = []
    for (const [key, set] of this.byKey) {
      for (const pid of set) out.push({ triggerKey: key, pluginId: pid })
    }
    return out
  }

  /** Test helper — drop every registered trigger and forget the activator. */
  reset(): void {
    this.byKey.clear()
    this.byPlugin.clear()
    this.inflight.clear()
    this.activator = null
  }
}

// Process-wide singleton, mirroring the `slotRegistry` / `uriHandlerRegistry`
// pattern. Registries import this directly; ExtensionHost wires the activator
// on construction.
export const activationTriggers = new ActivationTriggers()
