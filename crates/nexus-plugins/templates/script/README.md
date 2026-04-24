# {{plugin-name}}

{{description}}

A sandboxed community plugin for the Nexus shell. This plugin runs inside a
null-origin iframe and talks to the host over `postMessage`; it cannot touch
the shell's DOM, filesystem, or network directly — only through the
capability-gated surface on `SandboxedPluginContext`.

## Build

```sh
pnpm install
pnpm build
```

This produces `index.js` — a self-contained bundle that the Nexus shell loads
as the plugin entry point. The TypeScript source (`index.ts`) and manifest
(`plugin.json`) are the only files an author normally edits; everything else
is build scaffolding.

## Install

Drop the plugin directory (or at minimum the built bundle plus `plugin.json`)
into the shell's plugin directory:

```sh
mkdir -p ~/.nexus-shell/plugins/{{plugin-id}}
cp index.js plugin.json ~/.nexus-shell/plugins/{{plugin-id}}/
```

Restart the shell; the plugin appears in the "Running Extensions" settings
tab once it finishes activating. Use `nexus plugin list --shell` to confirm
the install.

To uninstall, run `nexus plugin remove {{plugin-id}}`.

## Extending

The full API surface lives in `@nexus/extension-api` — see the exported
`SandboxedPluginContext` for available `commands`, `views`, `notifications`,
`storage`, `statusBar`, `kernel.invoke`, and friends. The scaffold wires up
one command and one panel view; extend `activate(ctx)` with whatever your
plugin needs.

Declare any capabilities your plugin requires in `plugin.json` under
`capabilities` (for example `["UiNotify", "Storage"]`). The host denies calls
that aren't explicitly listed.
