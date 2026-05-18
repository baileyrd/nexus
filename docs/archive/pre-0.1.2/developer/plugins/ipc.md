# IPC: calling other plugins

IPC is the one mechanism every Nexus plugin uses to reach the rest of
the system. The shape is always the same:

```
context.ipc.call(plugin_id, command, args) -> Promise<Json>
```

CLI, TUI, MCP server, the desktop shell, core plugins, and community
plugins all use this one path. There's no special-case "shell IPC" or
"core-only API".

## A first call

```ts
import type { PluginContext } from '@nexus/extension-api';

async function listNotes(ctx: PluginContext) {
  const notes = await ctx.ipc.call(
    'com.nexus.storage',
    'list_notes',
    { prefix: 'projects/' }
  );
  return notes as Array<{ path: string; title: string }>;
}
```

The result is parsed JSON. Cast it to your expected shape (or, better,
validate with a runtime schema check — see [Testing](testing.md)).

## Capability requirement

`ipc.call` is a HIGH-risk capability. Your manifest must declare it:

```json
"capabilities": ["ipc.call"]
```

Without it, every `ctx.ipc.call(...)` rejects with
`CapabilityDenied`.

The targeted plugin's own capabilities still apply. Calling
`com.nexus.storage:write_note` triggers the storage plugin's
`fs.write` check — which it already holds. You're not borrowing the
target's permissions; you're triggering work it's authorized to do.

## Discovering commands

There's no central command registry by design — each plugin documents
its own. The discoverable surfaces:

- **Built-in core plugins**: see
  [`../../ipc-schemas.md`](../../ipc-schemas.md) for the schema
  generation policy and pointers to
  `packages/nexus-extension-api/src/generated/ipc/` (TypeScript
  bindings) and `crates/nexus-bootstrap/schemas/ipc/` (JSON Schema).
- **Other community plugins**: read the plugin's `README.md` or
  manifest contributions.
- **At runtime**: `nexus plugin list --format json` shows installed
  plugins; the IPC surface is currently not introspectable beyond
  what each plugin documents.

## Common targets

A reference of what most plugins call into:

| Plugin id | Useful commands |
|---|---|
| `com.nexus.storage` | `list_notes`, `read_note`, `create_note`, `update_note`, `delete_note`, `search`, `backlinks`, `outgoing_links`, `graph_neighbors` |
| `com.nexus.ai` | `ask` (RAG), `chat`, `embed`, `complete` |
| `com.nexus.editor` | `open`, `insert_at_cursor`, `decorate_block`, `register_slash_command` |
| `com.nexus.terminal` | `open_session`, `run`, `list_sessions` |
| `com.nexus.git` | `status`, `log`, `diff`, `blame` |
| `com.nexus.skills` | `list`, `render` |
| `com.nexus.workflow` | `list`, `run` |
| `com.nexus.theme` | `current`, `set`, `list` |

This is illustrative, not exhaustive. The authoritative listing is
the generated TypeScript types; import them for autocompletion:

```ts
import type { StorageIpc } from '@nexus/extension-api/ipc';

const notes: StorageIpc.ListNotesResponse =
  await ctx.ipc.call('com.nexus.storage', 'list_notes', {});
```

## Exposing your own commands

A plugin contributes commands by registering handlers in `activate`:

```ts
activate(ctx: PluginContext) {
  ctx.ipc.handle('greet', async (args: { name: string }) => {
    return { greeting: `Hello, ${args.name}!` };
  });
}
```

Now any other plugin (with `ipc.call`) can:

```ts
const r = await ctx.ipc.call('com.example.hello', 'greet', { name: 'World' });
// r === { greeting: 'Hello, World!' }
```

Handlers receive whatever JSON the caller sent. **Validate every
input** — never trust the args shape. A malformed call should return
an error, not crash. The kernel turns thrown exceptions into
`PluginCrashedDuringCall` errors with the underlying message.

## Error handling

IPC errors are typed:

| Error | Meaning |
|---|---|
| `PluginNotFound` | The target plugin isn't installed or enabled |
| `CommandNotFound` | The plugin exists but doesn't handle that command |
| `CapabilityDenied` | The caller doesn't have a needed capability |
| `InvalidArgs` | Args failed schema validation at the target |
| `Timeout` | The call exceeded the kernel's per-call timeout |
| `PluginCrashedDuringCall` | The target panicked or threw |

Both the JS and Rust error types carry the underlying message. In
Rust, the error is `IpcError` from `nexus_plugin_api::error`. As of
the recent change, `PluginCrashedDuringCall` includes a `reason`
string with the underlying error.

## Async semantics

- Calls are **async**. The kernel dispatches to the target's task
  pool; the caller awaits the response.
- There's a **per-call timeout** (default 30 s; configurable per
  call). If it expires, the caller gets `Timeout` but the target may
  still complete.
- Calls are **independent** — the kernel doesn't serialize calls into
  the same target. Targets that need exclusive access must use their
  own locking.

## When to use IPC vs. events

- **IPC** = request / response. You want an answer.
- **Events** = fire and forget. You're announcing something happened.

If you're tempted to use events because "the IPC target might not be
loaded," that's not a real concern: calling an inactive plugin
activates it (if its manifest lists `onCommand:` or matching
activation events). Use IPC.

See [Events](events.md) for the pub/sub side.

## Looking under the hood

If you want to understand the dispatcher itself (relevant for core
plugin authors):

- `crates/nexus-kernel/src/context_impl.rs` — the kernel's
  `PluginContext::ipc_call` implementation.
- `crates/nexus-plugins/src/loader.rs` — how community plugins are
  dispatched into.
- `crates/nexus-plugin-api/src/error.rs` — the `IpcError` type.
- [`../../ipc-schemas.md`](../../ipc-schemas.md) — schema generation
  and the drift check that keeps Rust + TS in sync.
