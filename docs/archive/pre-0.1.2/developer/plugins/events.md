# Events: pub/sub on the kernel bus

The kernel runs a typed pub/sub event bus. Plugins subscribe to
topics they care about and publish topics other plugins subscribe to.
Use events for **announcements**; use IPC for **requests** ([IPC](ipc.md)).

## Subscribing

```ts
import type { PluginContext, FilesChangedEvent } from '@nexus/extension-api';

activate(ctx: PluginContext) {
  ctx.events.subscribe('files:changed', (e: FilesChangedEvent) => {
    console.log(`${e.path} ${e.kind}`); // 'created' | 'modified' | 'deleted'
  });
}
```

Subscription requires no capability. Handlers can be sync or async;
the bus awaits async handlers in serial per topic.

A handler that throws is logged and the subscription **continues** —
one bad handler doesn't unhook future deliveries. (For a hard-fail
subscriber, set up your own try/catch.)

The subscription is automatically released on `deactivate`. To
unsubscribe early:

```ts
const off = ctx.events.subscribe('files:changed', handler);
// ...later
off();
```

## Publishing

```ts
ctx.events.publish('hello:greeted', { name: 'World', at: Date.now() });
```

Publishing requires the `events.publish` capability. The bus delivers
to every subscriber asynchronously.

Publish payloads should be JSON-serializable plain objects. Functions,
class instances, and circular references are not supported.

## Topic vocabulary

Topic names are colon-separated: `<domain>:<event>`. The domain is
typically a plugin id segment.

Core topics published by the kernel and built-in plugins:

| Topic | Payload | Published by |
|---|---|---|
| `files:changed` | `{ path, kind: 'created' \| 'modified' \| 'deleted' }` | `com.nexus.storage` |
| `files:opened` | `{ path }` | `com.nexus.editor` |
| `files:closed` | `{ path }` | `com.nexus.editor` |
| `editor:change` | `{ path, transaction }` | `com.nexus.editor` |
| `editor:cursorMoved` | `{ path, line, ch }` | `com.nexus.editor` |
| `workspace:opened` | `{ forgePath }` | shell |
| `workspace:closed` | `{ forgePath }` | shell |
| `workspace:layoutChanged` | `{ ... }` | shell |
| `ai:streamStart` | `{ sessionId }` | `com.nexus.ai` |
| `ai:streamChunk` | `{ sessionId, text }` | `com.nexus.ai` |
| `ai:streamDone` | `{ sessionId, response }` | `com.nexus.ai` |
| `plugin:enabled` / `plugin:disabled` | `{ id }` | kernel |

Authoritative payload shapes: the generated TypeScript event types in
[`packages/nexus-extension-api/src/generated/NexusEvent.ts`](../../../packages/nexus-extension-api/src/generated/NexusEvent.ts)
(or imports from `@nexus/extension-api`). Cross-reference
[`../../shell/event-bus.md`](../../shell/event-bus.md).

## Defining your own topics

Pick a domain prefix that matches your plugin id:

```ts
ctx.events.publish('com.example.hello:greeted', { name });
```

Document the payload shape in your plugin's README so subscribers
know what to expect. There's no central schema registry; the
convention is "publisher owns the topic shape".

## Activation events

A plugin can activate **on** an event:

```json
"activation": ["onEvent:files:changed"]
```

The first time `files:changed` fires after Nexus boots, your plugin
loads, runs `activate`, and your subscription is registered. Earlier
firings are missed — `onEvent` is not a replay.

## Patterns

### Debouncing high-frequency events

`editor:change` fires on every keystroke. Don't do work inside the
handler:

```ts
let pending: ReturnType<typeof setTimeout> | null = null;

ctx.events.subscribe('editor:change', (e) => {
  if (pending) clearTimeout(pending);
  pending = setTimeout(() => doExpensiveWork(e), 250);
});
```

### Cross-plugin coordination

Use events to **announce** that something happened, IPC to **ask**
for something to happen.

```
Plugin A:                                   Plugin B:
  ctx.events.publish('hello:greeted', …)    ctx.events.subscribe('hello:greeted', …)

  Plugin A:                                  Plugin B:
  await ctx.ipc.call('com.x.b', 'render', …) ctx.ipc.handle('render', …)
```

If you find yourself doing `events.publish('please-do-X', …)` and
expecting B to handle it, you want IPC.

### Filtering at the source

If you only care about markdown files, filter in the handler — there's
no kernel-side filter:

```ts
ctx.events.subscribe('files:changed', (e) => {
  if (!e.path.endsWith('.md')) return;
  // ...
});
```

A future bus version may add server-side filters.

## Limits

- **Ordering**: events are delivered in publish order, but handlers
  for **different topics** may interleave.
- **Backpressure**: there isn't any. A slow async handler can build a
  queue. Don't let handlers block.
- **Cross-frontend delivery**: in the desktop shell, events are
  delivered both to in-process Rust subscribers and to the Tauri
  renderer over the bridge. Payloads cross a JSON boundary; very
  large payloads (> 1 MB) are slow.
- **No replay**: the bus has no history. A subscriber that joins
  after a publish doesn't see it.

## See also

- [`../../shell/event-bus.md`](../../shell/event-bus.md)
  — naming conventions and shell-side event flow.
- [IPC](ipc.md) — the request/response sibling.
- [Lifecycle](lifecycle.md) — `onEvent:` activation.
