/**
 * Guest-side bootstrap for a sandboxed community plugin (WI-30c).
 *
 * This file runs **inside** the null-origin iframe the host spawns via
 * `SandboxOrchestrator` (WI-30b). It does not create an iframe; it
 * assumes one already hosts it. The plugin bundle imports
 * `bootstrapSandboxedPlugin` from `@nexus/extension-api`, hands in its
 * default-exported {@link SandboxedPlugin}, and control flow proceeds:
 *
 *   1. Post a `handshake` frame to `window.parent` (plugin-to-host).
 *   2. Wait for the host's `handshake` reply (host-to-plugin). A non-
 *      empty `error` on that frame means the host refused us; we bail.
 *   3. Build a {@link SandboxedPluginContext} proxy whose methods
 *      marshal arguments into `request` envelopes and await correlated
 *      `response` envelopes.
 *   4. Call `plugin.activate(ctx)`.
 *   5. Dispatch inbound `event` envelopes to subscribed handlers.
 *   6. On host `dispose` frame (or `beforeunload`), invoke
 *      `plugin.deactivate?()` and post a `dispose` ack.
 *
 * The runtime is deliberately small: registrations live on the host,
 * which auto-disposes them on teardown (see design §5.5). The guest's
 * only persistent state is the correlation map + subscription map.
 *
 * Phase 3c Wave 1 reconciliation note:
 *   Envelope shape matches `./protocol.ts` exactly (WI-30b canonical).
 *   `handshake` / `request` / `response` / `event` / `dispose` are the
 *   only `kind`s on the wire. Ping/pong (if ever needed) rides on
 *   `event` with method `sandbox.ping` — there is no dedicated `sys`
 *   kind anymore.
 */

import type { Disposable, PanelNode, PlatformAPI } from '../index';
import type {
  ActivityBarItemConfig,
  SandboxedPluginContext,
  StatusBarItemConfig,
  StatusBarItemHandle,
} from './context';
import type { SandboxedPlugin } from './plugin';
import {
  SANDBOX_PROTOCOL_VERSION,
  isRpcEnvelope,
  type HandshakeAccept,
  type HandshakeHello,
  type RpcEnvelope,
} from './protocol';

// Node-ish environments (tests) won't have `crypto.randomUUID`; fall back
// to a Math.random UUIDv4. Not security-critical — ids only need to be
// unique within the iframe's own correlation map.
function uuidv4(): string {
  const g = globalThis as { crypto?: { randomUUID?: () => string } };
  if (g.crypto && typeof g.crypto.randomUUID === 'function') {
    return g.crypto.randomUUID();
  }
  // Basic RFC-4122 v4 fallback.
  const r = (): number => (Math.random() * 16) | 0;
  const hex = (n: number): string => n.toString(16).padStart(2, '0');
  const bytes = Array.from({ length: 16 }, r);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const h = bytes.map(hex).join('');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

/** Minimal window contract the runtime depends on. */
type RuntimeWindow = {
  parent: { postMessage(msg: unknown, targetOrigin: string): void };
  addEventListener: (type: string, listener: (ev: unknown) => void) => void;
  removeEventListener: (type: string, listener: (ev: unknown) => void) => void;
};

/**
 * Bootstrap entry for a sandboxed plugin.
 *
 * Author exports their {@link SandboxedPlugin} as default and then
 * invokes `bootstrapSandboxedPlugin(plugin)` as the module's top-level
 * side effect. The host's `srcdoc` template dynamic-imports the bundle;
 * the side-effect call wires everything up.
 *
 * Returns nothing. All lifecycle is signalled via postMessage.
 */
export function bootstrapSandboxedPlugin(plugin: SandboxedPlugin): void {
  const win = globalThis as unknown as RuntimeWindow;

  const pending = new Map<string, PendingRequest>();
  const eventSubscriptions = new Map<
    string,
    (topic: string, payload: unknown) => void
  >();
  const commandHandlers = new Map<string, (...args: unknown[]) => unknown>();
  const uriHandlers = new Map<
    string,
    (url: URL) => void | Promise<void>
  >();
  const panelRenderers = new Map<string, () => PanelNode>();

  let accepted: HandshakeAccept | null = null;
  let deactivated = false;
  const handshakeNonce = uuidv4();

  // ── wire primitives ───────────────────────────────────────────────────

  const post = (env: RpcEnvelope): void => {
    // targetOrigin "*" is the only workable value — the host is at a
    // privileged origin we cannot name from "null", and the host
    // authenticates us by `event.source` identity (§3 threat 7).
    win.parent.postMessage(env, '*');
  };

  const request = (method: string, payload: unknown): Promise<unknown> => {
    return new Promise((resolve, reject) => {
      const id = uuidv4();
      pending.set(id, { resolve, reject });
      post({
        id,
        direction: 'plugin-to-host',
        kind: 'request',
        method,
        payload,
      });
    });
  };

  /** Fire-and-forget request variant — caller does not await a response. */
  const fireAndForget = (method: string, payload: unknown): void => {
    const id = uuidv4();
    post({
      id,
      direction: 'plugin-to-host',
      kind: 'request',
      method,
      payload,
    });
  };

  // ── context proxy ─────────────────────────────────────────────────────

  const buildContext = (accept: HandshakeAccept): SandboxedPluginContext => {
    // Helper: register a cleanup that fires once — mirrors the
    // idempotent-unsub pattern in `shell/src/host/PluginAPI.ts:253-287`.
    const onceDispose = (fn: () => void): Disposable => {
      let disposed = false;
      return () => {
        if (disposed) return;
        disposed = true;
        try {
          fn();
        } catch {
          /* swallow — dispose must not throw */
        }
      };
    };

    const platform: PlatformAPI = {
      fs: {
        readText: (path) => request('platform.fs.readText', { path }) as Promise<string>,
        writeText: (path, content) =>
          request('platform.fs.writeText', { path, content }) as Promise<void>,
        readDir: (path) =>
          request('platform.fs.readDir', { path }) as Promise<
            import('../index').PlatformDirEntry[]
          >,
        exists: (path) => request('platform.fs.exists', { path }) as Promise<boolean>,
        mkdir: (path, options) =>
          request('platform.fs.mkdir', {
            path,
            recursive: options?.recursive,
          }) as Promise<void>,
        remove: (path) => request('platform.fs.remove', { path }) as Promise<void>,
        rename: (from, to) =>
          request('platform.fs.rename', { from, to }) as Promise<void>,
      },
      dialog: {
        // TS needs the overload to line up — cast through unknown.
        openFile: ((options?: unknown) =>
          request('platform.dialog.openFile', { options })) as PlatformAPI['dialog']['openFile'],
        openDirectory: ((options?: unknown) =>
          request('platform.dialog.openDirectory', { options })) as PlatformAPI['dialog']['openDirectory'],
        saveFile: (options) =>
          request('platform.dialog.saveFile', { options }) as Promise<string | null>,
      },
      window: {
        minimize: () => request('platform.window.minimize', {}) as Promise<void>,
        toggleMaximize: () =>
          request('platform.window.toggleMaximize', {}) as Promise<void>,
        close: () => request('platform.window.close', {}) as Promise<void>,
        isMaximized: () =>
          request('platform.window.isMaximized', {}) as Promise<boolean>,
        onResize: async (handler) => {
          const handlerSub = uuidv4();
          eventSubscriptions.set(handlerSub, () => handler());
          await request('platform.window.onResize', { handlerSub });
          return onceDispose(() => {
            eventSubscriptions.delete(handlerSub);
            postDispose(handlerSub);
          });
        },
      },
      shell: {
        openExternal: (target) =>
          request('platform.shell.openExternal', { target }) as Promise<void>,
      },
      net: {
        request: (req) =>
          request('platform.net.request', req) as ReturnType<PlatformAPI['net']['request']>,
      },
    };

    const ctx: SandboxedPluginContext = {
      pluginId: accept.pluginInstanceId.split('#')[0] ?? accept.pluginInstanceId,

      commands: {
        register: (id, handler) => {
          const handlerSub = uuidv4();
          commandHandlers.set(handlerSub, handler);
          fireAndForget('commands.register', { id, handlerSub });
          return onceDispose(() => {
            commandHandlers.delete(handlerSub);
            postDispose(handlerSub);
          });
        },
        execute: (id, ...args) =>
          request('commands.execute', { id, args }),
      },

      kernel: {
        invoke: <T = unknown>(
          pluginId: string,
          commandId: string,
          args?: unknown,
          timeoutMs?: number,
        ) =>
          request('kernel.invoke', {
            pluginId,
            commandId,
            args,
            timeoutMs,
          }) as Promise<T>,
        on: async <T = unknown>(
          topicPrefix: string,
          handler: (topic: string, payload: T) => void,
        ) => {
          const handlerSub = uuidv4();
          eventSubscriptions.set(handlerSub, (topic, payload) =>
            handler(topic, payload as T),
          );
          await request('kernel.on', { topicPrefix, handlerSub });
          return onceDispose(() => {
            eventSubscriptions.delete(handlerSub);
            postDispose(handlerSub);
          });
        },
      },

      platform,

      events: {
        on: <T = unknown>(event: string, handler: (payload: T) => void) => {
          const handlerSub = uuidv4();
          eventSubscriptions.set(handlerSub, (_topic, payload) =>
            handler(payload as T),
          );
          // Fire-and-forget subscribe: the host's subscription-id is the
          // one we just generated; the ack is advisory. The returned
          // Disposable is sync to match the type contract.
          fireAndForget('events.on', { event, handlerSub });
          return onceDispose(() => {
            eventSubscriptions.delete(handlerSub);
            postDispose(handlerSub);
          });
        },
        emit: (event, payload) =>
          fireAndForget('events.emit', { event, payload }),
      },

      storage: {
        get: (key) => request('storage.get', { key }) as Promise<string | null>,
        set: (key, value) => request('storage.set', { key, value }) as Promise<void>,
        delete: (key) => request('storage.delete', { key }) as Promise<void>,
        list: (prefix) => request('storage.list', { prefix }) as Promise<string[]>,
      },

      notifications: {
        show: (notification) =>
          request('notifications.show', { notification }) as Promise<void>,
      },

      context: {
        set: (key, value) => request('context.set', { key, value }) as Promise<void>,
        get: (key) => request('context.get', { key }),
        evaluate: (expression) =>
          request('context.evaluate', { expression }) as Promise<boolean>,
      },

      views: {
        registerPanel: (viewId, render) => {
          const renderSub = uuidv4();
          panelRenderers.set(renderSub, render);
          fireAndForget('views.registerPanel', {
            viewId,
            // Slot resolution is host-side today; the guest defaults to
            // the plugin's declared viewId slot. WI-30d refines this.
            slot: viewId,
            renderSub,
          });
          return onceDispose(() => {
            panelRenderers.delete(renderSub);
            postDispose(renderSub);
          });
        },
      },

      input: {
        prompt: (message, placeholder) =>
          request('input.prompt', { message, placeholder }) as Promise<string | null>,
        confirm: (message) => request('input.confirm', { message }) as Promise<boolean>,
      },

      uri: {
        register: (scheme, handler) => {
          const handlerSub = uuidv4();
          uriHandlers.set(handlerSub, handler);
          fireAndForget('uri.register', { scheme, handlerSub });
          return onceDispose(() => {
            uriHandlers.delete(handlerSub);
            postDispose(handlerSub);
          });
        },
      },

      activityBar: {
        addItem: (config: ActivityBarItemConfig) => {
          fireAndForget('activityBar.addItem', { config });
          return onceDispose(() =>
            fireAndForget('activityBar.removeItem', { id: config.id }),
          );
        },
        removeItem: (id) => fireAndForget('activityBar.removeItem', { id }),
      },

      statusBar: {
        createItem: async (config: StatusBarItemConfig) => {
          await request('statusBar.createItem', { config });
          const handle: StatusBarItemHandle = {
            id: config.id,
            update: (patch) =>
              request('statusBar.update', { id: config.id, patch }) as Promise<void>,
            dispose: () =>
              request('statusBar.dispose', { id: config.id }) as Promise<void>,
          };
          return handle;
        },
      },
    };

    return ctx;
  };

  // ── message dispatch ──────────────────────────────────────────────────

  const postDispose = (subscriptionId: string): void => {
    post({
      id: subscriptionId,
      direction: 'plugin-to-host',
      kind: 'dispose',
      payload: { subscriptionId },
    });
  };

  const handleResponse = (env: RpcEnvelope): void => {
    const entry = pending.get(env.id);
    if (!entry) return;
    pending.delete(env.id);
    if (env.error) {
      entry.reject(env.error);
    } else {
      entry.resolve(env.payload);
    }
  };

  const handleEvent = (env: RpcEnvelope): void => {
    // Events correlate via `id` (= the host's echo of our handlerSub).
    const subId = env.id;
    const handler = eventSubscriptions.get(subId);
    if (!handler) return;
    // The host-side event envelope carries `{ topic, payload }` for
    // kernel.on-style subscriptions, or the bare payload for
    // events.on / window.onResize. Duck-type the payload shape.
    const payload = env.payload;
    if (
      payload &&
      typeof payload === 'object' &&
      'topic' in (payload as Record<string, unknown>)
    ) {
      const wrapped = payload as { topic?: unknown; payload?: unknown };
      const topic = typeof wrapped.topic === 'string' ? wrapped.topic : env.method ?? '';
      try {
        handler(topic, wrapped.payload);
      } catch (err) {
        console?.error?.('[sandbox] subscription handler threw', err);
      }
      return;
    }
    try {
      handler(env.method ?? '', payload);
    } catch (err) {
      console?.error?.('[sandbox] subscription handler threw', err);
    }
  };

  const handleRequest = (env: RpcEnvelope): void => {
    // Host → guest dispatch. Only a few methods flow this way:
    // command-handler invocation, uri-handler invocation, and
    // panel-render requests. Everything else is guest-initiated.
    const respondOk = (result: unknown): void => {
      post({
        id: env.id,
        direction: 'plugin-to-host',
        kind: 'response',
        method: env.method,
        payload: result,
      });
    };
    const respondErr = (message: string): void => {
      post({
        id: env.id,
        direction: 'plugin-to-host',
        kind: 'response',
        method: env.method,
        error: {
          kind: 'dispatch_failed',
          message,
          retryable: false,
          pluginId: accepted?.pluginInstanceId,
          method: env.method,
        },
      });
    };

    const args = (env.payload ?? {}) as Record<string, unknown>;

    try {
      if (env.method === 'dispatch.command') {
        const handlerSub =
          typeof args.handlerSub === 'string' ? args.handlerSub : undefined;
        const rawArgs = Array.isArray(args.args) ? (args.args as unknown[]) : [];
        const handler = handlerSub ? commandHandlers.get(handlerSub) : undefined;
        if (!handler) return respondErr('unknown handlerSub');
        Promise.resolve(handler(...rawArgs)).then(respondOk, (err) =>
          respondErr(String(err)),
        );
        return;
      }

      if (env.method === 'dispatch.uri') {
        const handlerSub =
          typeof args.handlerSub === 'string' ? args.handlerSub : undefined;
        const url = typeof args.url === 'string' ? args.url : undefined;
        const handler = handlerSub ? uriHandlers.get(handlerSub) : undefined;
        if (!handler || !url) {
          return respondErr('unknown uri handler');
        }
        Promise.resolve(handler(new URL(url))).then(
          () => respondOk(undefined),
          (err) => respondErr(String(err)),
        );
        return;
      }

      if (env.method === 'views.render') {
        const renderSub =
          typeof args.renderSub === 'string' ? args.renderSub : undefined;
        const render = renderSub ? panelRenderers.get(renderSub) : undefined;
        if (!render) return respondErr('unknown renderSub');
        respondOk(render());
        return;
      }

      respondErr(`unsupported host-initiated method: ${env.method ?? '<none>'}`);
    } catch (err) {
      respondErr(String(err));
    }
  };

  const handleHandshake = (env: RpcEnvelope): void => {
    if (accepted) return;
    if (env.error) {
      // Host refused us. Nothing to do — iframe will be torn down.
      return;
    }
    const payload = env.payload as Partial<HandshakeAccept> | undefined;
    if (
      !payload ||
      typeof payload.pluginInstanceId !== 'string' ||
      !Array.isArray(payload.methods)
    ) {
      return;
    }
    accepted = {
      protocolVersion: SANDBOX_PROTOCOL_VERSION,
      pluginInstanceId: payload.pluginInstanceId,
      methods: payload.methods as ReadonlyArray<string>,
      nonce: typeof payload.nonce === 'string' ? payload.nonce : handshakeNonce,
    };
    const ctx = buildContext(accepted);
    Promise.resolve()
      .then(() => plugin.activate(ctx))
      .catch((err) => {
        // Surface activation failures as a dispose frame rather than a
        // handshake reject — the host has already accepted us.
        post({
          id: uuidv4(),
          direction: 'plugin-to-host',
          kind: 'dispose',
          error: {
            kind: 'dispatch_failed',
            message: `plugin activate threw: ${String(err)}`,
            retryable: false,
            pluginId: accepted?.pluginInstanceId,
          },
        });
      });
  };

  const handleDispose = (env: RpcEnvelope): void => {
    // Host signalled teardown. Run deactivate and ack.
    runDeactivate().finally(() => {
      post({
        id: env.id,
        direction: 'plugin-to-host',
        kind: 'dispose',
      });
    });
  };

  const runDeactivate = async (): Promise<void> => {
    if (deactivated) return;
    deactivated = true;
    try {
      await plugin.deactivate?.();
    } catch (err) {
      console?.error?.('[sandbox] deactivate threw', err);
    }
    // Reject any still-pending calls so plugin promises unblock for GC.
    for (const [, entry] of pending) {
      entry.reject(new Error('plugin unloaded'));
    }
    pending.clear();
    eventSubscriptions.clear();
    commandHandlers.clear();
    uriHandlers.clear();
    panelRenderers.clear();
  };

  const messageListener = (ev: unknown): void => {
    const data = (ev as { data?: unknown }).data;
    if (!isRpcEnvelope(data)) return;
    // Ignore our own echoes and any frames not destined for the guest.
    if (data.direction !== 'host-to-plugin') return;
    // #196 / R13 — sandbox watchdog. The host fires `sandbox.ping` on
    // an interval (`SandboxOrchestrator.startWatchdog`) and tears the
    // plugin down after `maxMissedPongs` consecutive pings without a
    // pong reply. Until this auto-pong landed, a healthy plugin that
    // simply didn't subscribe to `sandbox.ping` events was indistinguishable
    // from a wedged guest and got false-crashed. Replying inline here
    // — before `handleEvent` lookups — means the heartbeat is independent
    // of plugin code and survives even when the plugin's own event
    // loop is busy on a synchronous activate.
    if (data.kind === 'event' && data.method === 'sandbox.ping') {
      try {
        post({
          id: data.id,
          direction: 'plugin-to-host',
          kind: 'event',
          method: 'sandbox.pong',
          payload: { ts: Date.now() },
        });
      } catch {
        /* best-effort; missed pong falls through to watchdog */
      }
      return;
    }
    switch (data.kind) {
      case 'handshake':
        handleHandshake(data);
        return;
      case 'response':
        handleResponse(data);
        return;
      case 'event':
        handleEvent(data);
        return;
      case 'request':
        handleRequest(data);
        return;
      case 'dispose':
        handleDispose(data);
        return;
    }
  };

  win.addEventListener('message', messageListener);
  win.addEventListener('beforeunload', () => {
    void runDeactivate();
  });

  // Kick off the handshake. The host is listening on a single global
  // `message` handler (design §5.1) and will reply with an accept
  // (kind=handshake, direction=host-to-plugin) or reject (same, with
  // `error` populated).
  const hello: HandshakeHello = {
    protocolVersion: SANDBOX_PROTOCOL_VERSION,
    apiVersion: 1,
    nonce: handshakeNonce,
  };
  post({
    id: handshakeNonce,
    direction: 'plugin-to-host',
    kind: 'handshake',
    payload: hello,
  });
}
