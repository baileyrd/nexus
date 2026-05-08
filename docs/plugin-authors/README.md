# Plugin Authors

The curated path for writing plugins for Nexus. Two paths converge here:
sandboxed JS/TS plugins (the modern default) and pure-Rust WASM plugins
(legacy, still supported).

## Start here

1. **[`quickstart.md`](quickstart.md)** — Scaffold, build, install, and run
   your first plugin. ~10 minutes from zero to a registered command and a
   panel view.

2. **[`../../shell/docs/writing-a-plugin.md`](../../shell/docs/writing-a-plugin.md)**
   — In-depth reference: manifest fields, activation events, sandbox
   model, capability declarations, slot system, and a worked word-count
   example. Read this once you have a scaffold building.

3. **[`../../shell/docs/plugin-api.md`](../../shell/docs/plugin-api.md)** —
   The full `@nexus/extension-api` surface: commands, views, context,
   events, config, statusBar, notifications, fs, storage.

## Architectural background

These ADRs explain why plugins look the way they do. Read whichever apply
to the kind of plugin you're writing.

- **[ADR 0002](../adr/0002-hierarchical-capability-strings.md)** — the
  capability taxonomy your manifest declares against. Every gated API
  call requires the matching capability.
- **[ADR 0015](../adr/0015-iframe-sandbox-plugin-runtime.md)** — the
  null-origin iframe + `postMessage` RPC sandbox model for JS/TS
  plugins. Explains what crosses the boundary and what doesn't.
- **[ADR 0016](../adr/0016-microkernel-native-vs-wasm-plugin-split.md)** —
  when to write a native (Rust) plugin vs a WASM/JS one.
- **[ADR 0005](../adr/0005-single-dispatch-handler-ids.md)** — the
  single-dispatch handler-ID convention; relevant if you're routing
  IPC calls through `kernel.invoke`.

## Templates

- **[`../templates/community-plugin/README.md`](../templates/community-plugin/README.md)** —
  capability-gated WASM plugin scaffold (`nexus plugin scaffold --template community`).
- **[`../templates/core-plugin/README.md`](../templates/core-plugin/README.md)** —
  maximum-trust core plugin scaffold (`nexus plugin scaffold --template core`).

## Where capability strings come from

The capability enum is defined in Rust at `crates/nexus-plugin-api` and
generated into TypeScript at
`packages/nexus-extension-api/src/generated/`. The TS file is the
authoritative listing for plugin authors — start there if you need to
know what `Capability` values are valid.
