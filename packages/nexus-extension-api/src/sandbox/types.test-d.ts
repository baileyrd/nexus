/**
 * Compile-only type smoke tests for the sandbox surface (WI-30c).
 *
 * No runtime — `tsc --noEmit` failure indicates a broken contract.
 * There is no test runner wired to this file; it participates in the
 * normal `pnpm --filter nexus-shell typecheck` (the shell imports the
 * package via tsconfig `paths`) and in the package's own
 * `tsc -p tsconfig.json --noEmit`.
 *
 * If/when Vitest-style type assertions land in the workspace, swap the
 * hand-rolled `expectType` for the canonical helper.
 *
 * Phase 3c Wave 1 reconciliation note:
 *   Envelope shape switched to the WI-30b canonical discriminants
 *   (handshake / request / response / event / dispose). The
 *   previously-abbreviated three-letter kinds are gone.
 */

import type { Disposable, PanelNode, PlatformAPI } from '../index';
import type {
  ActivityBarItemConfig,
  SandboxedPluginContext,
  StatusBarItemConfig,
  StatusBarItemHandle,
} from './context';
import type { SandboxedPlugin } from './plugin';
import type {
  HandshakeAccept,
  HandshakeHello,
  RpcDirection,
  RpcEnvelope,
  RpcErrorEnvelope,
  RpcKind,
} from './protocol';
import { SANDBOX_PROTOCOL_VERSION, isRpcEnvelope } from './protocol';
import { bootstrapSandboxedPlugin } from './runtime';

declare function expectType<T>(value: T): void;
declare const _never: never;

// ─── Protocol constants ─────────────────────────────────────────────────────

expectType<1>(SANDBOX_PROTOCOL_VERSION);

// ─── Envelope discriminants ─────────────────────────────────────────────────

declare const env: RpcEnvelope;
expectType<string>(env.id);
expectType<RpcDirection>(env.direction);
expectType<RpcKind>(env.kind);

switch (env.kind) {
  case 'handshake':
  case 'request':
  case 'response':
  case 'event':
  case 'dispose':
    expectType<RpcEnvelope>(env);
    break;
}

// `error` is a union of RpcErrorEnvelope | IpcErrorEnvelope; at the
// RpcEnvelope surface it's just `RpcResponseError | undefined`.
if (env.error) {
  // Narrow: at minimum both branches carry `kind` + `message`.
  expectType<string>(env.error.kind);
  expectType<string>(env.error.message);
}

// ─── Handshake payloads ─────────────────────────────────────────────────────

declare const hello: HandshakeHello;
declare const accept: HandshakeAccept;
expectType<number>(hello.protocolVersion);
expectType<number>(hello.apiVersion);
expectType<string>(hello.nonce);
expectType<string>(accept.pluginInstanceId);
expectType<ReadonlyArray<string>>(accept.methods);

// ─── Error envelope ─────────────────────────────────────────────────────────

declare const rpcErr: RpcErrorEnvelope;
expectType<RpcErrorEnvelope['kind']>(rpcErr.kind);
expectType<boolean>(rpcErr.retryable);

// ─── Type guards ────────────────────────────────────────────────────────────

declare const maybeEnv: unknown;
if (isRpcEnvelope(maybeEnv)) {
  expectType<RpcEnvelope>(maybeEnv);
}

// ─── Context shape ──────────────────────────────────────────────────────────

declare const ctx: SandboxedPluginContext;
expectType<string>(ctx.pluginId);

// Sync → async conversions documented in context.ts
expectType<Promise<string | null>>(ctx.storage.get('k'));
expectType<Promise<void>>(ctx.storage.set('k', 'v'));
expectType<Promise<void>>(ctx.notifications.show({ message: 'hi' }));
expectType<Promise<unknown>>(ctx.context.get('k'));
expectType<Promise<boolean>>(ctx.context.evaluate('true'));
expectType<Promise<StatusBarItemHandle>>(
  ctx.statusBar.createItem({
    id: 'x',
    slot: 'left',
    priority: 0,
    text: 'hi',
  } satisfies StatusBarItemConfig),
);

// PlatformAPI already async — unchanged.
expectType<PlatformAPI>(ctx.platform);
expectType<Promise<string>>(ctx.platform.fs.readText('/tmp/x'));

// Views: registerPanel takes PanelNode renderer, not React.
const disposePanel = ctx.views.registerPanel('v', () => ({
  type: 'heading',
  value: 'x',
  level: 1,
} satisfies PanelNode));
expectType<Disposable>(disposePanel);

// Commands: register is sync (returns Disposable); execute is async.
expectType<Disposable>(
  ctx.commands.register('id', (..._args: unknown[]): unknown => undefined),
);
expectType<Promise<unknown>>(ctx.commands.execute('id'));

// Kernel.on returns Promise<Disposable> (was already async).
declare const kernelUnsub: Promise<Disposable>;
expectType<Promise<Disposable>>(ctx.kernel.on('topic.', (_t, _p) => {}));
void kernelUnsub;

// ActivityBar config type exported for plugin authors.
declare const abItem: ActivityBarItemConfig;
expectType<string>(abItem.id);
expectType<'top' | 'bottom' | undefined>(abItem.placement);

// ─── Plugin shape ───────────────────────────────────────────────────────────

const samplePlugin: SandboxedPlugin = {
  activate(_c: SandboxedPluginContext) {
    /* no-op */
  },
  deactivate() {
    /* no-op */
  },
};
expectType<SandboxedPlugin>(samplePlugin);

// ─── Runtime entry ──────────────────────────────────────────────────────────

expectType<(p: SandboxedPlugin) => void>(bootstrapSandboxedPlugin);

// Prove the exports line up: treat a never-reached use-site as a sink
// so tsc --noUnusedLocals doesn't complain about `_never`.
export const _sink: typeof _never | undefined = undefined;
