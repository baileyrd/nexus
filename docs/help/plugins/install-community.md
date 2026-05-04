# Installing community plugins

Community plugins are user-installed extensions. They run in a wasmtime
sandbox and can only reach the host through capability-gated IPC.

> **Status:** A built-in plugin **marketplace** with discovery and
> one-click install is on the roadmap (work item WI-44). Today, you
> install plugins from a local file or URL.

## Install from a local `.wasm`

```bash
nexus plugin install ./my-plugin.wasm
```

You'll be shown the plugin manifest (id, name, version, requested
capabilities) and asked to approve. On approval, the plugin is copied
into `<forge>/.forge/plugins/` and activated.

## Install from a URL

```bash
nexus plugin install https://example.com/foo.wasm
```

Same approval flow. The downloaded artifact is verified against the
publisher's signature if one is configured (see
[ADR for plugin manifest signing](../../adr/) for details).

## From the shell

Open the **Plugins** panel:

1. Click **Install plugin…**.
2. Pick a local file or paste a URL.
3. Review the capability list.
4. Approve or cancel.

## Manage installed plugins

```bash
nexus plugin list                  # all installed plugins
nexus plugin enable com.x.foo
nexus plugin disable com.x.foo
nexus plugin uninstall com.x.foo
nexus plugin settings com.x.foo    # edit per-plugin config
nexus plugin reset com.x.foo       # wipe state, keep installation
```

Same operations are available from the Plugins panel in the shell.

## Capabilities

A plugin manifest lists the capabilities it needs:

| Capability | What it grants |
|---|---|
| `fs.read` | Read files in the forge |
| `fs.write` | Create / modify files |
| `ipc.call:<plugin>/<command>` | Call another plugin's IPC handler |
| `events.publish` | Publish on the event bus |
| `events.subscribe` | Subscribe to events |
| `kv.read`, `kv.write` | Use plugin KV storage |
| `network.fetch:<host>` | HTTP fetch to specific hosts |
| `process.spawn` | Spawn subprocesses |

Wildcards: `ipc.call:com.nexus.storage/*` grants every storage IPC.
Always grant the narrowest set the plugin actually needs.

## Update a plugin

Reinstall it. The new manifest's capability list is shown again — you
re-approve any new permissions, but already-granted ones carry over.

## Safe mode

If a community plugin breaks Nexus, restart with
`nexus --safe-mode desktop`. Community plugins won't load; you can then
disable or uninstall the offender.

## Where they live

```
<forge>/.forge/plugins/
├── com.example.foo/
│   ├── plugin.json        manifest
│   ├── plugin.wasm        sandboxed code
│   └── settings.json      per-plugin config
└── com.example.bar/
    └── ...
```

Per-forge: a plugin installed in one forge is not active in another.
