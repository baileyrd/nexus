# Building your own plugin

Nexus has a quickstart that gets you from zero to a working plugin in
about ten minutes. This page is a high-level orientation; for the
hands-on path, follow
[`docs/plugin-authors/quickstart.md`](../../plugin-authors/quickstart.md).

## Scaffold

```bash
nexus plugin scaffold \
  --template script \
  --id com.example.hello \
  --name "Hello Plugin"
```

You get a directory with:

- `plugin.json` — manifest (id, name, version, capabilities,
  contributions)
- `src/index.ts` — entry point
- `package.json`, `tsconfig.json`
- A build script that produces `plugin.wasm`

## The manifest

```json
{
  "id": "com.example.hello",
  "name": "Hello",
  "version": "0.1.0",
  "main": "plugin.wasm",
  "capabilities": ["ipc.call:com.nexus.storage/*"],
  "activation": ["onCommand:hello.sayHi"],
  "contributes": {
    "commands": [
      { "id": "hello.sayHi", "title": "Hello: Say Hi" }
    ]
  }
}
```

## Entry point

```ts
import { activate, registerCommand, notify } from '@nexus/extension-api';

activate(() => {
  registerCommand('hello.sayHi', async (ctx) => {
    notify({ message: 'Hi!' });
  });
});
```

## Build and install

```bash
pnpm build              # produces plugin.wasm
nexus plugin install .  # installs from the current directory
```

## Develop with hot reload

`nexus plugin install --watch .` rebuilds and reloads on every file
change. Restart of Nexus is not required.

## What you get from the API

The TypeScript surface (`@nexus/extension-api`) covers:

- **Commands** — register and invoke palette commands
- **Views** — contribute panels, status-bar items
- **Editor** — slash commands, decorations, MDX components
- **IPC** — `context.ipc.call(pluginId, command, args)` to call any
  registered handler in any other plugin (capability-permitting)
- **Events** — `subscribe(topic, handler)` / `publish(topic, payload)`
- **Storage** — `context.fs` (file ops), `context.kv` (key-value)
- **Settings** — declare a JSON schema; settings UI is auto-generated
- **Notifications** — toasts and alerts

Full API reference: [`shell/docs/plugin-api.md`](../../../shell/docs/plugin-api.md).

## Capabilities

Declare every kernel-mediated operation. The user sees the list at
install time. See
[ADR 0002](../../adr/0002-capability-system.md) for the design and the
full capability vocabulary.

## Testing

The scaffold includes a Vitest harness with a mock kernel context:

```ts
import { describe, it, expect } from 'vitest';
import { mockContext } from '@nexus/extension-api/testing';

it('greets', async () => {
  const ctx = mockContext();
  await activate(ctx);
  await ctx.commands.invoke('hello.sayHi');
  expect(ctx.notifications.last()?.message).toBe('Hi!');
});
```

## Publishing

For now: distribute the `.wasm` and manifest through any channel
(GitHub releases, your own server). Once the marketplace ships
(WI-44) you'll publish there with a `nexus plugin publish` command.
