# Testing your plugin

> **Story.** Nexus does not ship a `@nexus/extension-api/testing`
> entrypoint or a `mockContext` factory. Every shipped plugin under
> `shell/src/plugins/nexus/**` and `crates/nexus-*/` is tested with the
> language's built-in test runner against hand-rolled fakes of the
> `NexusPluginContext` / `KernelAPI` surfaces it actually uses. Build
> the smallest fake your code needs; don't reach for a generic harness
> that doesn't exist.

Two runners cover the two plugin shapes Nexus supports today:

- **Script plugins (TypeScript)** — `node --test` via the `node:test`
  module. The shell workspace already wires it up; tests live next to
  the code as `*.test.ts`.
- **Native plugins (Rust)** — `cargo test -p <crate>`, with `tokio::test`
  for async surface and `tempfile::TempDir` for forge-shaped fixtures.

## Project setup (script plugins)

Tests live next to the file they cover and are discovered by `node:test`:

```bash
pnpm --filter nexus-shell test            # all shell tests
pnpm --filter nexus-shell test -- --watch # watch mode (Node flag)
```

A scaffolded plugin gets the same toolchain when it ships inside the
shell workspace. Out-of-tree script plugins choose their own runner;
the recipes below transpose cleanly to Vitest or Jest if you prefer.

## A first test

```ts
// shell/src/plugins/nexus/hello/hello.test.ts
import { test } from 'node:test';
import assert from 'node:assert/strict';

import type { KernelAPI, NexusPluginContext } from '../../../types/plugin.ts';
import { plugin } from './index.ts';

interface FakeKernel extends KernelAPI {
  calls: Array<{ pluginId: string; commandId: string; args: unknown }>;
}

function makeFakeKernel(): FakeKernel {
  const calls: FakeKernel['calls'] = [];
  return {
    calls,
    async invoke<T>(pluginId, commandId, args): Promise<T> {
      calls.push({ pluginId, commandId, args: args ?? {} });
      return undefined as T;
    },
    // …implement only the methods this test exercises
  } as FakeKernel;
}

test('plugin starts cleanly', async () => {
  const ctx = makeFakeContext();
  await plugin.onStart?.(ctx);
  assert.equal(ctx.disposables.size > 0, true);
});
```

The pattern is **build the minimum fake your code touches, then assert
on its recorded state**. `shell/src/plugins/nexus/status/statusStore.test.ts`
is a worked example of a `KernelAPI` fake with recorded calls and
canned responses.

## Faking subsystems

`NexusPluginContext` is the host-supplied object the plugin sees at
runtime. There is no factory; the host wires it up inside the shell.
For unit tests, build narrow fakes scoped to the subsystem you exercise:

| Subsystem (real) | Test approach |
|---|---|
| `ipc.call(plugin, command, args)` | Hand-rolled fake with a `calls: […]` array and a `responses: Map<key, value>` for canned returns. |
| `events.emit(topic, payload)` | Push to a recorded array; subscribers fan out via a `Map<topic, Set<handler>>`. |
| `settings.get(key)` / `onChange` | In-memory `Map`; trigger `onChange` callbacks synchronously. |
| `editor.register*` / contributions | Record the contribution in a list; assert what your `onStart` registered. |
| `ui.notify(level, msg)` | Push to a `notifications: NoticeRecord[]` array. |

When the real surface evolves, the compiler will flag drift the moment
you re-`import type` the contract.

## Testing IPC interactions

Plugins reach storage / AI / git / etc. through `ctx.ipc.call(...)`.
Tests substitute a `KernelAPI` whose `invoke` returns canned values:

```ts
test('save dispatches storage.write_file', async () => {
  const kernel = makeFakeKernel();
  kernel.responses.set('com.nexus.storage::write_file', { ok: true });
  const ctx = makeFakeContext({ kernel });

  await plugin.onStart?.(ctx);
  await ctx.commands.invoke?.('hello.save');

  assert.deepEqual(kernel.calls.map(c => c.commandId), ['write_file']);
});
```

Unregistered targets should error in the fake the same way the
real dispatcher errors (`PluginNotFound` / `CommandNotFound`).

## Testing event flows

Drive subscribers by emitting against your fake:

```ts
const ctx = makeFakeContext();
await plugin.onStart?.(ctx);
ctx.events.emit('com.nexus.storage::file_changed', {
  path: 'a.md',
  kind: 'modified',
});
```

To verify what your plugin **published**, record into an array inside
the fake's `emit` and assert against it.

## Testing capability denials

The real host throws a `CapabilityDenied` error before the kernel
dispatches an `ipc.call` for an ungranted capability. Mirror that in
the fake by gating `invoke` on a configured capability set:

```ts
const ctx = makeFakeContext({ deniedCapabilities: new Set(['fs.write']) });
await assert.rejects(
  () => ctx.commands.invoke?.('hello.save'),
  /CapabilityDenied/,
);
```

## Testing settings

```ts
const ctx = makeFakeContext({ settings: { 'hello.greeting': 'Howdy' } });
await plugin.onStart?.(ctx);
// plugin reads hello.greeting; uses 'Howdy'.

ctx.settings.setValue?.('hello.greeting', 'Hi'); // triggers onChange
```

## Integration testing against a real runtime

Beyond unit tests, you can run a plugin end-to-end inside a temp forge
by driving the `nexus` CLI:

```ts
import { spawnSync } from 'node:child_process';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';

const forge = await mkdtemp(`${tmpdir()}/nexus-test-`);
process.env.NEXUS_FORGE_PATH = forge;
spawnSync('nexus', ['forge', 'init', forge]);
spawnSync('nexus', ['plugin', 'install', './']);
const result = spawnSync(
  'nexus',
  ['plugin', 'call', 'com.example.hello', 'greet', '--arg', 'name=Ada'],
);
// assert on result.stdout
```

This catches integration bugs the fake can't see (manifest typos,
missing capabilities, ABI version mismatch).

## What hand-rolled fakes don't simulate

- **Sandbox boundaries.** Tests run in-process; postMessage
  serialization quirks and WASM ABI edges only show up in integration.
- **Concurrency.** Synchronous fake dispatch hides races. Use
  `setTimeout(0)` or real promise scheduling in the fake to model
  microtask ordering.
- **Persistence.** Fakes reset per test. Upgrade scenarios need an
  integration run against a real forge.

## See also

- [Lifecycle](lifecycle.md) — what to call in tests (`onInit` /
  `onStart` / `onStop`).
- [IPC](ipc.md), [Events](events.md), [Settings](settings.md) —
  subsystems the fake must shadow.
- `packages/nexus-extension-api/src/index.ts` — authoritative source
  of the `NexusPluginContext` shape you're faking.
