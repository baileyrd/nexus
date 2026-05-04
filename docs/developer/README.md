# Nexus Developer Docs

You're here to **build something that runs inside Nexus**: a community
plugin, a core plugin, a theme, an MDX component, an editor extension,
or an MCP-side integration. This is the hub.

If you're a **user** trying to learn what Nexus does, see
[`../help/`](../help/README.md). If you're contributing to **Nexus core**
itself (kernel, bootstrap, shell, etc.), see
[`../README.md`](../README.md) and [`../../CONTRIBUTING.md`](../../CONTRIBUTING.md).

## Pick your path

- **I want a working "hello world" plugin in 10 minutes** →
  [Getting started](getting-started.md)
- **I want to understand the architecture before I build** →
  [Architecture primer](architecture-primer.md)
- **I want to know what plugins can actually do** →
  [Plugins / overview](plugins/overview.md)
- **I'm writing a theme** →
  [Themes / build a theme](themes/build-a-theme.md)
- **I want the API surface, not a tutorial** →
  [Reference](reference.md)

## Plugins

- [Overview: core vs. community](plugins/overview.md)
- [Manifest spec](plugins/manifest.md)
- [Lifecycle and activation](plugins/lifecycle.md)
- [Capabilities reference](plugins/capabilities.md)
- [IPC: calling other plugins](plugins/ipc.md)
- [Events: pub/sub on the kernel bus](plugins/events.md)
- [Settings schemas](plugins/settings.md)
- [Testing your plugin](plugins/testing.md)
- [Publishing and distribution](plugins/publishing.md)

## Editor extensions

- [Editor extension model](editor/overview.md)
- [Contributing slash commands](editor/slash-commands.md)
- [Contributing MDX components](editor/mdx-components.md)

## User interface

- [Views, panels, and slots](ui/views-and-slots.md)
- [Commands and keybindings](ui/commands-and-keybindings.md)
- [Context keys and `when` clauses](ui/context-keys.md)

## Themes

- [Build a theme](themes/build-a-theme.md)
- [CSS variable reference](themes/css-variables.md)

## Core plugins (Rust)

- [Authoring a core plugin](core-plugins/authoring.md)

## Reference

- [Reference index](reference.md) — pointers to authoritative API,
  IPC, capability, and ADR sources.

---

## A note on tiers

Nexus has **two plugin tiers**, and the choice shapes everything else:

- **Core plugins** — native Rust, compiled into the binary, full host
  access. Authored by the Nexus team or explicitly trusted partners.
  See [Core plugins / authoring](core-plugins/authoring.md).
- **Community plugins** — sandboxed (WASM or iframe-JS), capability-
  gated, user-installable. This is what most third-party developers
  build. See [Plugins / overview](plugins/overview.md).

Both tiers use the same extension API and contribute through the same
mechanisms. The difference is the trust boundary: a community plugin
can never do something the user hasn't approved a capability for, and
a misbehaving one can be sandbox-killed without taking down Nexus.
