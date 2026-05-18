# Capabilities reference

A **capability** is a named permission your plugin requests in its
manifest and the kernel checks on every gated operation. The list
below is the **canonical and complete** vocabulary. The Rust source
of truth is `Capability` in
[`crates/nexus-plugin-api/src/capability.rs`](../../../crates/nexus-plugin-api/src/capability.rs).

There are 22 capabilities (the original 14 plus 8 `ai.*` variants added
by [ADR 0022](../../adr/0022-per-handler-ai-capabilities.md)). 6 are
classified **HIGH risk** and require explicit user approval at install
time.

## Filesystem

| String | Risk | Grants |
|---|---|---|
| `fs.read` | normal | Read files within the forge root |
| `fs.write` | normal | Create / modify / delete files within the forge root |
| `fs.read.external` | **HIGH** | Read files outside the forge root |
| `fs.write.external` | **HIGH** | Write files outside the forge root |

Forge-internal access is normal-risk because the user already trusts
plugins with their notes (they installed the plugin into this forge).
External access is high-risk because it lets a plugin reach into
arbitrary parts of the user's machine.

## Network

| String | Risk | Grants |
|---|---|---|
| `net.http` | **HIGH** | Outbound HTTP / HTTPS to any host |
| `net.http.localhost` | normal | Outbound HTTP only to `localhost` / `127.0.0.1` |

`net.http.localhost` is meant for plugins that talk to a locally-run
service (Ollama, a developer's REPL, etc.). It does not expose the
user's IP or data to the internet.

## Process

| String | Risk | Grants |
|---|---|---|
| `process.spawn` | **HIGH** | Spawn child processes |

This includes terminal sessions, build commands, anything that exec's.
Almost no plugin should request this; if you find yourself wanting
it, consider whether your need can be met by an IPC call into
`com.nexus.terminal` (which already has the capability and a
user-mediated UX around it).

## IPC

| String | Risk | Grants |
|---|---|---|
| `ipc.call` | **HIGH** | Call IPC commands on other plugins |
| `events.publish` | normal | Publish on the kernel event bus |

The default behavior is **subscribe-only** — a plugin can listen to
events without any capability. Publishing requires `events.publish`,
because publishes can drive other plugins' activation.

`ipc.call` is high-risk because it can chain into capabilities the
caller doesn't itself hold. (The target's capabilities still apply,
but the caller can trigger expensive operations.)

## Storage

| String | Risk | Grants |
|---|---|---|
| `kv.read` | normal | Read the plugin's own KV namespace |
| `kv.write` | normal | Write the plugin's own KV namespace |
| `db.query` | normal | Query SQLite tables registered by the plugin |
| `db.write` | normal | Write to SQLite tables registered by the plugin |

KV is a per-plugin key-value store; you can never read another
plugin's KV. The DB capabilities apply to tables your plugin has
registered through `com.nexus.database` — you can't reach into the
forge's index DB.

## UI

| String | Risk | Grants |
|---|---|---|
| `ui.notify` | normal | Show toast notifications to the user |

Most other UI surfaces (panels, status-bar items, command palette)
don't require an explicit capability — contributing them is what your
plugin is *for*. `ui.notify` is gated because notifications can be
spammed in a way that damages the user experience.

## Approval flow

When you `nexus plugin install`, the user sees:

```
com.example.hello v1.0.0 wants:
  ✓ ui.notify          (show notifications)
  ⚠ ipc.call           (call other plugins)         [HIGH]
  ⚠ net.http           (make any HTTP request)      [HIGH]

[Approve]  [Cancel]
```

HIGH-risk capabilities are highlighted. The user can approve or
cancel; per-capability decline is **not** offered (it's all-or-
nothing) because partial grants tend to land plugins in surprising
unsupported states. If the user wants to limit what you can do, they
shouldn't install you.

After approval, the grant is persisted at
`<forge>/.forge/plugins/<id>/grants.json`. Reinstalling with a new
manifest re-prompts only for newly added capabilities.

## At runtime

A capability check that fails returns an `IpcError::CapabilityDenied`
to the caller. Well-written plugins handle this gracefully:

```ts
try {
  await ctx.fs.write('out.md', body);
} catch (e) {
  if (isCapabilityError(e)) {
    ctx.ui.notify({
      message: 'Hello plugin needs fs.write permission. Re-install to grant.',
      level: 'error',
    });
    return;
  }
  throw e;
}
```

## Picking the right capabilities

Two principles:

1. **Narrowest set.** Don't request `fs.write.external` if you only
   write into the forge. Don't request `net.http` if you only talk to
   `localhost`.
2. **Declare upfront.** Don't request a capability "just in case".
   Users distrust plugins with long permission lists, and asking for
   more than you use is a smell.

If a plugin's job genuinely needs a HIGH capability, say so in the
manifest's `description` so the install dialog reads sensibly.

## Adding a new capability

If you're contributing to Nexus core and need a new capability,
extend the `Capability` enum in
[`crates/nexus-plugin-api/src/capability.rs`](../../../crates/nexus-plugin-api/src/capability.rs).
Add the canonical string to `as_str` / `from_str`, list it in `ALL`,
and decide if it's HIGH-risk in `is_high_risk`. The `ts-export`
feature regenerates the TypeScript binding automatically.

ADR: [`../../adr/0002-hierarchical-capability-strings.md`](../../adr/0002-hierarchical-capability-strings.md).
