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
  HandshakeAcceptData,
  HandshakeHelloData,
  RpcEnvelope,
  RpcEventEnvelope,
  RpcRequestEnvelope,
  RpcResponseErrEnvelope,
  RpcResponseOkEnvelope,
  RpcSystemEnvelope,
} from './protocol';
import { SANDBOX_PROTOCOL_VERSION, isRpcEnvelope } from './protocol';
import { bootstrapSandboxedPlugin } from './runtime';

declare function expectType<T>(value: T): void;
declare const _never: never;

// ─── Protocol constants ─────────────────────────────────────────────────────

expectType<1>(SANDBOX_PROTOCOL_VERSION);

// ─── Envelope discriminants ─────────────────────────────────────────────────

declare const env: RpcEnvelope;
switch (env.kind) {
  case 'req':
    expectType<RpcRequestEnvelope>(env);
    expectType<string>(env.method);
    break;
  case 'res':
    if (env.ok) expectType<RpcResponseOkEnvelope>(env);
    else expectType<RpcResponseErrEnvelope>(env);
    break;
  case 'evt':
    expectType<RpcEventEnvelope>(env);
    expectType<'h2g'>(env.dir);
    break;
  case 'sys':
    expectType<RpcSystemEnvelope>(env);
    break;
}

// ─── Handshake payloads ─────────────────────────────────────────────────────

declare const hello: HandshakeHelloData;
declare const accept: HandshakeAcceptData;
expectType<number>(hello.protocolVersion);
expectType<string>(accept.pluginInstanceId);
expectType<ReadonlyArray<string>>(accept.methods);

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
