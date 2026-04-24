// shell/src/plugins/community/hello-world/index.js
//
// WI-30e — hello-world migrated to the sandbox contract.
//
// This file is the self-contained bundle the SandboxOrchestrator
// dynamic-imports inside the null-origin iframe (see
// shell/src/host/sandbox/SandboxOrchestrator.ts:buildSandboxSrcDoc).
// Because Phase 3c has not wired a plugin bundler yet (WI-30e §4 in
// the design doc), we cannot yet write this file as an idiomatic
// `import { bootstrapSandboxedPlugin } from '@nexus/extension-api'`
// module — the iframe has no module resolver for bare specifiers.
//
// As a stepping stone, this file hand-rolls the sandbox protocol
// directly:
//   1. postMessage a `handshake` hello with the current protocol
//      version (keep in sync with
//      packages/nexus-extension-api/src/sandbox/protocol.ts —
//      SANDBOX_PROTOCOL_VERSION = 1).
//   2. Wait for the host's `handshake` accept.
//   3. Register a `hello.greet` command handler + a `hello.panel`
//      PanelNode renderer.
//   4. Relay host→plugin `dispatch.command` / `views.render` /
//      `dispose` requests to local handlers.
//
// The companion `index.ts` file is the idiomatic SandboxedPlugin source
// that SHOULD become the author experience once a bundler lands. That
// file is the spec for what the real toolchain must emit into this
// slot; the two must stay semantically aligned so the bundler cutover
// is a drop-in.
//
// Manifest notes:
//   - `sandboxed: true`     — routes through SandboxOrchestrator.
//   - `apiVersion: 1`       — matches PLUGIN_API_VERSION in the shell.
//   - `capabilities:
//       ["UiNotify"]`       — guards `notifications.show` (the only
//                             host-side surface this plugin reaches).
//
// What was lost vs. the pre-WI-30e legacy plugin:
//   - The status-bar item (`api.statusBar.createItem`). The sandbox
//     `StatusBar` surface is available (see
//     nexus-extension-api/src/sandbox/context.ts) but exercising it
//     here would add a second capability (`UiStatusBar`) and widen the
//     consent prompt; the migration is intentionally minimal so it can
//     double as a smoke test for the dispatch.command round-trip.
//     Restoring it is a one-liner once the end-to-end path is green.

// ── Protocol constants (keep in lockstep with protocol.ts) ─────────────────
const SANDBOX_PROTOCOL_VERSION = 1;

// ── UUID fallback (iframes in tests lack crypto.randomUUID) ────────────────
function uuidv4() {
  const g = globalThis;
  if (g.crypto && typeof g.crypto.randomUUID === 'function') {
    return g.crypto.randomUUID();
  }
  const r = () => (Math.random() * 16) | 0;
  const hex = (n) => n.toString(16).padStart(2, '0');
  const bytes = Array.from({ length: 16 }, r);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const h = bytes.map(hex).join('');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

// ── Guest state ────────────────────────────────────────────────────────────
const pending = new Map();              // correlation id -> { resolve, reject }
const commandHandlers = new Map();      // handlerSub -> fn
const panelRenderers = new Map();       // renderSub   -> () => PanelNode

const handshakeNonce = uuidv4();
let accepted = false;

function post(env) {
  // targetOrigin '*' is the only workable value — host is at a
  // privileged origin we cannot name from a null-origin iframe; the
  // host authenticates us by `event.source` identity.
  window.parent.postMessage(env, '*');
}

function request(method, payload) {
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
}

function fireAndForget(method, payload) {
  post({
    id: uuidv4(),
    direction: 'plugin-to-host',
    kind: 'request',
    method,
    payload,
  });
}

// ── Plugin logic ───────────────────────────────────────────────────────────
async function activate() {
  await request('notifications.show', {
    notification: {
      message: 'Hello from the sandbox!',
      type: 'info',
      duration: 3000,
    },
  });

  const greetSub = uuidv4();
  commandHandlers.set(greetSub, async () => {
    const name = await request('input.prompt', {
      message: 'Your name?',
      placeholder: 'e.g. Ada',
    });
    if (name) {
      await request('notifications.show', {
        notification: {
          message: `Hi ${name}!`,
          type: 'info',
          duration: 3000,
        },
      });
    }
  });
  fireAndForget('commands.register', {
    id: 'hello.greet',
    handlerSub: greetSub,
  });

  const panelSub = uuidv4();
  panelRenderers.set(panelSub, () => ({
    type: 'vstack',
    gap: 8,
    children: [
      { type: 'heading', value: 'Hello', level: 2 },
      { type: 'text', value: 'Click the button to greet someone.' },
      { type: 'button', label: 'Greet', commandId: 'hello.greet' },
    ],
  }));
  fireAndForget('views.registerPanel', {
    viewId: 'hello.panel',
    slot: 'hello.panel',
    renderSub: panelSub,
  });
}

// ── Inbound dispatch ───────────────────────────────────────────────────────
function respondOk(id, method, payload) {
  post({
    id,
    direction: 'plugin-to-host',
    kind: 'response',
    method,
    payload,
  });
}
function respondErr(id, method, message) {
  post({
    id,
    direction: 'plugin-to-host',
    kind: 'response',
    method,
    error: {
      kind: 'dispatch_failed',
      message,
      retryable: false,
      method,
    },
  });
}

function handleHostRequest(env) {
  const args = env.payload || {};
  if (env.method === 'dispatch.command') {
    const handler = commandHandlers.get(args.handlerSub);
    if (!handler) return respondErr(env.id, env.method, 'unknown handlerSub');
    Promise.resolve(handler(...(Array.isArray(args.args) ? args.args : []))).then(
      (v) => respondOk(env.id, env.method, v),
      (err) => respondErr(env.id, env.method, String(err && err.message || err)),
    );
    return;
  }
  if (env.method === 'views.render') {
    const render = panelRenderers.get(args.renderSub);
    if (!render) return respondErr(env.id, env.method, 'unknown renderSub');
    try { respondOk(env.id, env.method, render()); }
    catch (err) { respondErr(env.id, env.method, String(err && err.message || err)); }
    return;
  }
  respondErr(env.id, env.method, `unsupported host-initiated method: ${env.method}`);
}

function handleResponse(env) {
  const entry = pending.get(env.id);
  if (!entry) return;
  pending.delete(env.id);
  if (env.error) entry.reject(env.error);
  else entry.resolve(env.payload);
}

function handleHandshake(env) {
  if (accepted) return;
  if (env.error) return;
  const payload = env.payload || {};
  if (typeof payload.pluginInstanceId !== 'string') return;
  accepted = true;
  Promise.resolve().then(activate).catch((err) => {
    post({
      id: uuidv4(),
      direction: 'plugin-to-host',
      kind: 'dispose',
      error: {
        kind: 'dispatch_failed',
        message: `plugin activate threw: ${String(err && err.message || err)}`,
        retryable: false,
        pluginId: payload.pluginInstanceId,
      },
    });
  });
}

window.addEventListener('message', (ev) => {
  const data = ev.data;
  if (!data || typeof data !== 'object') return;
  if (data.direction !== 'host-to-plugin') return;
  switch (data.kind) {
    case 'handshake': handleHandshake(data); return;
    case 'response':  handleResponse(data);  return;
    case 'request':   handleHostRequest(data); return;
    case 'dispose':
      // Best-effort ack; no plugin-owned teardown yet.
      post({
        id: data.id,
        direction: 'plugin-to-host',
        kind: 'dispose',
      });
      return;
  }
});

// Kick off the handshake. The host's SandboxOrchestrator + router pair
// will reply with a `handshake` accept frame (direction: host-to-plugin).
post({
  id: handshakeNonce,
  direction: 'plugin-to-host',
  kind: 'handshake',
  payload: {
    protocolVersion: SANDBOX_PROTOCOL_VERSION,
    apiVersion: 1,
    nonce: handshakeNonce,
  },
});

// Default export so the SandboxOrchestrator's srcdoc template —
// which does `const bundle = await import(bundleUrl); plugin = bundle.default`
// — sees a valid plugin object. With the bootstrap side-effect above we
// don't strictly need `activate()` to be called again, but providing the
// shape keeps the bundler-less stepping stone honest: once a bundler
// lands, this file becomes a compiled form of `index.ts` and the
// `bootstrapSandboxedPlugin(plugin)` call will replace the inline
// side-effect driver.
export default {
  activate() { /* already wired via inline bootstrap above */ },
};
