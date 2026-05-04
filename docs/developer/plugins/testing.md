# Testing your plugin

A plugin is a `Plugin` object that takes a `PluginContext`. Both are
plain TypeScript shapes, so unit-testing is straightforward: build a
mock context, call your `activate`, and assert against what the
plugin did.

## Project setup

The scaffold ships with [Vitest](https://vitest.dev) wired in:

```bash
pnpm test                 # run once
pnpm test --watch         # watch mode
pnpm test --coverage      # with coverage
```

Tests live in `tests/` next to `src/`.

## A first test

```ts
// tests/hello.test.ts
import { describe, it, expect } from 'vitest';
import { mockContext } from '@nexus/extension-api/testing';
import { plugin } from '../src/index';

describe('hello plugin', () => {
  it('registers the sayHi command', async () => {
    const ctx = mockContext();
    await plugin.activate(ctx);

    expect(ctx.commands.list()).toContain('hello.sayHi');
  });

  it('greets the user', async () => {
    const ctx = mockContext({
      ui: { promptResponse: 'Ada' },
    });
    await plugin.activate(ctx);

    await ctx.commands.invoke('hello.sayHi');

    expect(ctx.ui.lastNotification()?.message).toBe('Hello, Ada!');
  });
});
```

## What `mockContext` gives you

`mockContext(opts?)` returns a `PluginContext` whose subsystems are
in-memory fakes. Each subsystem records what your plugin did and
exposes inspection helpers.

| Subsystem | Inspection helpers |
|---|---|
| `commands` | `list()`, `invoke(id, args?)`, `unregisterCount()` |
| `events`   | `published()`, `subscriptionsFor(topic)`, `emit(topic, payload)` |
| `ipc`      | `recordedCalls()`, `setHandler(plugin, command, fn)` |
| `kv`       | Real in-memory map; `dump()` to inspect |
| `fs`       | In-memory file system; `seed({ path: contents })` |
| `config`   | `setValue(key, val)`, `lastChanged()` |
| `ui`       | `lastNotification()`, `notifications()`, `promptResponse` |
| `env`      | `setEnv(key, val)` |

`opts` lets you preconfigure responses:

```ts
const ctx = mockContext({
  fs: { seed: { 'README.md': '# Hi' } },
  ipc: {
    handlers: {
      'com.nexus.storage:list_notes': async () => [{ path: 'a.md' }],
    },
  },
  ui: { promptResponse: 'Ada' },
  config: { values: { 'hello.greeting': 'Hi' } },
});
```

## Testing IPC

When your plugin calls `ctx.ipc.call(...)`, the mock looks up a
registered fake handler. Unregistered targets reject with
`PluginNotFound`:

```ts
ctx.ipc.setHandler('com.nexus.ai', 'ask', async (args) => {
  return { answer: `mock answer for: ${args.question}` };
});

await plugin.activate(ctx);
await ctx.commands.invoke('hello.askAi', { question: '?' });

expect(ctx.ipc.recordedCalls()).toEqual([
  { plugin: 'com.nexus.ai', command: 'ask', args: { question: '?' } },
]);
```

## Testing event subscribers

Use `ctx.events.emit` to drive subscribers:

```ts
await plugin.activate(ctx);
ctx.events.emit('files:changed', {
  path: 'a.md',
  kind: 'modified',
});

expect(/* whatever your subscriber did */).toBe(/* expected */);
```

To verify your plugin **published** something:

```ts
expect(ctx.events.published()).toContainEqual({
  topic: 'hello:greeted',
  payload: { name: 'Ada' },
});
```

## Testing capabilities

By default the mock context grants every capability. To assert your
plugin handles denied capabilities:

```ts
const ctx = mockContext({ capabilities: { deny: ['fs.write'] } });
await plugin.activate(ctx);

await expect(ctx.commands.invoke('hello.save')).rejects.toThrow(
  /CapabilityDenied/
);
```

## Testing settings

```ts
const ctx = mockContext({
  config: { values: { 'hello.greeting': 'Howdy' } },
});
await plugin.activate(ctx);
// plugin reads hello.greeting from ctx.config — uses 'Howdy'

ctx.config.setValue('hello.greeting', 'Hi');
// triggers any onChange subscriber
```

## Testing async flows

Use Vitest's `vi.advanceTimersByTime` for debounced handlers, and
`await Promise.resolve()` (or `vi.waitFor`) to flush microtasks
between `emit` and assertion.

## Integration testing

Beyond unit tests, you can run a plugin against a real Nexus runtime
in a temp forge:

```ts
import { spawn } from 'node:child_process';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';

const forge = await mkdtemp(`${tmpdir()}/nexus-test-`);
spawn('nexus', ['forge', 'init', forge]);
spawn('nexus', ['plugin', 'install', './'], { cwd: __dirname });

const result = spawn('nexus', ['plugin', 'call', 'com.example.hello', 'greet', '--arg', 'name=Ada']);
// assert on stdout
```

This is slower but catches integration bugs (manifest typos, missing
capabilities, etc.) the mock can't see.

## What the mock doesn't simulate

- **Sandbox boundaries.** The mock runs your code in the same process
  as the test. Sandbox-specific bugs (postMessage serialization,
  WASM ABI quirks) won't surface.
- **Concurrency.** The mock processes IPC and events synchronously
  per call. Race conditions won't reproduce.
- **Persistence.** The mock KV / fs / config reset per test. Run
  integration tests against a real forge for upgrade scenarios.

For sandbox-level testing, use the integration approach above.

## See also

- [Lifecycle](lifecycle.md) — what to call in tests (activate /
  deactivate).
- [IPC](ipc.md), [Events](events.md), [Settings](settings.md) —
  subsystems the mock fakes.
