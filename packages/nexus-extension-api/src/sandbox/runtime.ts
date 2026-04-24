/**
 * Guest-side bootstrap for a sandboxed community plugin (WI-30c).
 *
 * This file runs **inside** the null-origin iframe the host spawns via
 * `SandboxOrchestrator` (WI-30b). It does not create an iframe; it
 * assumes one already hosts it. The plugin bundle imports
 * `bootstrapSandboxedPlugin` from `@nexus/extension-api`, hands in its
 * default-exported {@link SandboxedPlugin}, and control flow proceeds:
 *
 *   1. Listen for an inbound `handshake/accept` from `window.parent`.
 *   2. Build a {@link SandboxedPluginContext} proxy whose methods
 *      marshal arguments into `req` envelopes and await correlated
 *      `res` envelopes.
 *   3. Call `plugin.activate(ctx)`.
 *   4. Dispatch inbound `evt` envelopes to subscribed handlers.
 *   5. On `unload` signal (host `sys:unload` or window `beforeunload`),
 *      invoke `plugin.deactivate?()` and respond `sys:unload` ack.
 *
 * The runtime is deliberately small: registrations live on the host,
 * which auto-disposes them on teardown (see design §5.5). The guest's
 * only persistent state is the correlation map + subscription map.
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
  type HandshakeAcceptData,
  type HandshakeHelloData,
  type RpcEnvelope,
  type RpcEventEnvelope,
  type RpcRequestEnvelope,
  type RpcResponseErrEnvelope,
  type RpcResponseOkEnvelope,
  type RpcSystemEnvelope,
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

  let accepted: HandshakeAcceptData | null = null;
  let deactivated = false;

  // ── wire primitives ───────────────────────────────────────────────────

  const post = (env: RpcEnvelope): void => {
    // targetOrigin "*" is the only workable value — the host is at a
    // privileged origin we cannot name from "null", and the host
    // authenticates us by `event.source` identity (§3 threat 7).
    win.parent.postMessage(env, '*');
  };

  const request = (method: string, args: unknown): Promise<unknown> => {
    return new Promise((resolve, reject) => {
      const id = uuidv4();
      pending.set(id, { resolve, reject });
      post({ id, dir: 'g2h', kind: 'req', method, args });
    });
  };

  // ── context proxy ─────────────────────────────────────────────────────

  const buildContext = (accept: HandshakeAcceptData): SandboxedPluginContext => {
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

    const fireAndForget = (method: string, args: unknown): void => {
      const id = uuidv4();
      post({ id, dir: 'g2h', kind: 'req', method, args });
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
          request('platform.fs.mkdir', { path, options }) as Promise<void>,
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
          const subscriptionId = uuidv4();
          eventSubscriptions.set(subscriptionId, () => handler());
          await request('platform.window.onResize', { subscriptionId });
          return onceDispose(() => {
            eventSubscriptions.delete(subscriptionId);
            fireAndForget('unsubscribe', { subscriptionId });
          });
        },
      },
      shell: {
        openExternal: (target) =>
          request('platform.shell.openExternal', { target }) as Promise<void>,
      },
    };

    const ctx: SandboxedPluginContext = {
      pluginId: accept.pluginId,

      commands: {
        register: (id, handler) => {
          const handlerId = uuidv4();
          commandHandlers.set(handlerId, handler);
          fireAndForget('commands.register', { id, handlerId });
          return onceDispose(() => {
            commandHandlers.delete(handlerId);
            fireAndForget('commands.unregister', { id, handlerId });
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
            targetId: pluginId,
            cmd: commandId,
            args,
            timeoutMs,
          }) as Promise<T>,
        on: async <T = unknown>(
          topicPrefix: string,
          handler: (topic: string, payload: T) => void,
        ) => {
          const subscriptionId = uuidv4();
          eventSubscriptions.set(subscriptionId, (topic, payload) =>
            handler(topic, payload as T),
          );
          await request('kernel.on', { topicPrefix, subscriptionId });
          return onceDispose(() => {
            eventSubscriptions.delete(subscriptionId);
            fireAndForget('unsubscribe', { subscriptionId });
          });
        },
      },

      platform,

      events: {
        on: <T = unknown>(event: string, handler: (payload: T) => void) => {
          const subscriptionId = uuidv4();
          eventSubscriptions.set(subscriptionId, (_topic, payload) =>
            handler(payload as T),
          );
          // Fire-and-forget subscribe: the host's subscription-id is the
          // one we just generated; the ack is advisory. The returned
          // Disposable is sync to match the type contract.
          fireAndForget('events.on', { event, subscriptionId });
          return onceDispose(() => {
            eventSubscriptions.delete(subscriptionId);
            fireAndForget('unsubscribe', { subscriptionId });
          });
        },
        emit: (event, payload) =>
          fireAndForget('events.emit', { event, payload }),
      },

      storage: {
        get: (key) => request('storage.get', { key }) as Promise<string | null>,
        set: (key, value) => request('storage.set', { key, value }) as Promise<void>,
        delete: (key) => request('storage.delete', { key }) as Promise<void>,
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
          panelRenderers.set(viewId, render);
          fireAndForget('views.registerPanel', { viewId });
          return onceDispose(() => {
            panelRenderers.delete(viewId);
            fireAndForget('views.unregisterPanel', { viewId });
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
          const handlerId = uuidv4();
          uriHandlers.set(handlerId, handler);
          fireAndForget('uri.register', { scheme, handlerId });
          return onceDispose(() => {
            uriHandlers.delete(handlerId);
            fireAndForget('uri.unregister', { scheme, handlerId });
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

  const handleResponse = (
    env: RpcResponseOkEnvelope | RpcResponseErrEnvelope,
  ): void => {
    const entry = pending.get(env.id);
    if (!entry) return;
    pending.delete(env.id);
    if (env.ok) entry.resolve(env.result);
    else entry.reject(env.error);
  };

  const handleEvent = (env: RpcEventEnvelope): void => {
    const handler = eventSubscriptions.get(env.subscriptionId);
    if (!handler) return;
    try {
      handler(env.topic, env.payload);
    } catch (err) {
      // Swallow — plugin handler errors must not propagate into the
      // runtime. The host will see them via heartbeat misses if they
      // jam the loop.
      console?.error?.('[sandbox] subscription handler threw', err);
    }
  };

  const handleRequest = (env: RpcRequestEnvelope): void => {
    // Host → guest dispatch. Only a few methods flow this way:
    // command-handler invocation, uri-handler invocation, and
    // panel-render requests. Everything else is guest-initiated.
    const respondOk = (result: unknown): void => {
      post({ id: env.id, dir: 'g2h', kind: 'res', ok: true, result });
    };
    const respondErr = (message: string): void => {
      post({
        id: env.id,
        dir: 'g2h',
        kind: 'res',
        ok: false,
        error: {
          kind: 'dispatch_failed',
          plugin_id: accepted?.pluginId ?? '',
          command: env.method,
          message,
          retryable: false,
        },
      });
    };

    try {
      if (env.method === 'dispatch.command') {
        const { handlerId, args } = (env.args ?? {}) as {
          handlerId?: string;
          args?: unknown[];
        };
        const handler = handlerId ? commandHandlers.get(handlerId) : undefined;
        if (!handler) return respondErr('unknown handlerId');
        Promise.resolve(handler(...(args ?? []))).then(respondOk, (err) =>
          respondErr(String(err)),
        );
        return;
      }

      if (env.method === 'dispatch.uri') {
        const { handlerId, url } = (env.args ?? {}) as {
          handlerId?: string;
          url?: string;
        };
        const handler = handlerId ? uriHandlers.get(handlerId) : undefined;
        if (!handler || typeof url !== 'string') {
          return respondErr('unknown uri handler');
        }
        Promise.resolve(handler(new URL(url))).then(
          () => respondOk(undefined),
          (err) => respondErr(String(err)),
        );
        return;
      }

      if (env.method === 'views.render') {
        const { viewId } = (env.args ?? {}) as { viewId?: string };
        const render = viewId ? panelRenderers.get(viewId) : undefined;
        if (!render) return respondErr('unknown viewId');
        respondOk(render());
        return;
      }

      respondErr(`unsupported host-initiated method: ${env.method}`);
    } catch (err) {
      respondErr(String(err));
    }
  };

  const handleSystem = (env: RpcSystemEnvelope): void => {
    switch (env.system) {
      case 'handshake/accept': {
        if (accepted) return;
        accepted = env.data as HandshakeAcceptData;
        const ctx = buildContext(accepted);
        Promise.resolve()
          .then(() => plugin.activate(ctx))
          .catch((err) => {
            post({
              id: uuidv4(),
              dir: 'g2h',
              kind: 'sys',
              system: 'handshake/reject',
              data: {
                error: {
                  kind: 'plugin_crashed',
                  plugin_id: accepted?.pluginId ?? '',
                  command: 'activate',
                  message: String(err),
                  retryable: false,
                },
              },
            });
          });
        return;
      }
      case 'handshake/reject': {
        // Host refused us. Nothing to do — iframe will be torn down.
        return;
      }
      case 'ping': {
        post({ id: env.id, dir: 'g2h', kind: 'sys', system: 'pong' });
        return;
      }
      case 'unload': {
        runDeactivate().finally(() => {
          post({ id: env.id, dir: 'g2h', kind: 'sys', system: 'unload' });
        });
        return;
      }
      default:
        // suspend/resume/pong/hello are not meaningful to receive guest-side.
        return;
    }
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
    switch (data.kind) {
      case 'res':
        handleResponse(data);
        return;
      case 'evt':
        handleEvent(data);
        return;
      case 'req':
        handleRequest(data);
        return;
      case 'sys':
        handleSystem(data);
        return;
    }
  };

  win.addEventListener('message', messageListener);
  win.addEventListener('beforeunload', () => {
    void runDeactivate();
  });

  // Kick off the handshake. The host is listening on a single global
  // `message` handler (design §5.1) and will respond with
  // `handshake/accept` on success.
  const helloData: HandshakeHelloData = {
    protocolVersion: SANDBOX_PROTOCOL_VERSION,
  };
  post({
    id: uuidv4(),
    dir: 'g2h',
    kind: 'sys',
    system: 'handshake/hello',
    data: helloData,
  });
}
