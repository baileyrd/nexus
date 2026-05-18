# Authoring a core plugin

A **core plugin** is a native Rust crate that registers with the
kernel at boot. It runs in-process with full host access, doesn't go
through the WASM/iframe sandbox, and is shipped as part of the Nexus
binary.

You're writing a core plugin if:

- You're upstreaming a new built-in subsystem to Nexus.
- The work needs raw filesystem / process / network access that no
  capability cleanly authorizes.
- Performance overhead of a sandbox is unacceptable.

Otherwise — write a [community plugin](../plugins/overview.md).

## The contract

Implement the `CorePlugin` trait from `nexus-plugins`. The real surface
is **synchronous, numeric-handler-dispatched**, and routes through a
manifest registered alongside the plugin — not the async string-keyed
`handle_ipc` shape older drafts of this doc described.

```rust
use std::sync::Arc;
use nexus_plugins::loader::{CorePlugin, CorePluginFuture, KernelPluginContext};
use nexus_plugin_api::error::PluginError;

// Numeric handler ids — opaque to callers, mapped via the manifest.
pub const HANDLER_SAY_HI: u32 = 1;

pub struct HelloPlugin {
    ctx: Option<Arc<KernelPluginContext>>,
}

impl HelloPlugin {
    pub fn new() -> Self {
        Self { ctx: None }
    }
}

impl CorePlugin for HelloPlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        // One-time setup. No `ctx` argument — wire_context (below)
        // delivered it before the kernel called on_init.
        Ok(())
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        // Subscribe to events, kick off background tasks.
        Ok(())
    }

    fn on_stop(&mut self) {
        // Graceful shutdown — flush state, cancel tasks.
        // `on_stop` returns nothing; failures should be logged, not propagated.
    }

    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_SAY_HI => {
                let name: String = serde_json::from_value(args.clone())
                    .map_err(|e| PluginError::SerializationFailed {
                        plugin_id: "com.example.hello".into(),
                        details: e.to_string(),
                    })?;
                Ok(serde_json::json!({ "greeting": format!("Hello, {name}!") }))
            }
            _ => Err(PluginError::CommandNotFound {
                plugin_id: "com.example.hello".into(),
                command: format!("handler {handler_id}"),
            }),
        }
    }

    /// Optional override — return a `Future` for handlers that need
    /// async work (HTTP, long-running disk I/O). Default impl returns
    /// `None`, which means the loader uses sync `dispatch`.
    fn dispatch_async(
        &mut self,
        _handler_id: u32,
        _args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        None
    }

    /// The kernel hands you a context **before** `on_init`. Stash it
    /// so handlers can publish events and call other plugins.
    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        self.ctx = Some(ctx);
    }
}
```

Lifecycle hooks — `on_init`, `on_start`, `on_stop` — map to the states
in [Lifecycle](../plugins/lifecycle.md). All three are **synchronous**:
the loader calls them on the kernel's lifecycle thread, so background
work belongs in a task spawned during `on_start`. `on_init` / `on_start`
returning `Err` halt the plugin (the bootstrap can choose to either
abort or skip-and-continue per `or_lifecycle_skip`); `on_stop` cannot
fail by design.

Identity (`id`, `name`, `version`, `trust_level`, `capabilities`) is
**not** carried on the trait — there is no `fn info(&self)`. Those fields
live in the [`PluginManifest`] you register alongside the plugin in
bootstrap (see next section). Authors who tried to override an `info`
method on this trait were following a draft that never shipped.

[`PluginManifest`]: ../../crates/nexus-plugins/src/manifest.rs

## Where to put the crate

A core plugin lives as its own crate in the workspace:

```
crates/nexus-hello/
├── Cargo.toml
└── src/
    ├── lib.rs           # impl CorePlugin
    └── ...
```

Add it to the workspace `Cargo.toml`:

```toml
[workspace]
members = [
    # ...existing crates...
    "crates/nexus-hello",
]
```

## Register in bootstrap

`crates/nexus-bootstrap/src/lib.rs` exposes two public entry points —
`build_cli_runtime(forge_root: PathBuf) -> Result<Runtime>` and
`build_tui_runtime(forge_root: PathBuf) -> Result<Runtime>`. Both
delegate to a private `register_core_plugins(loader, forge_root, event_bus)`
that is the single point wiring every core plugin into the kernel.
Adding a new core plugin means editing that private function — there
is no public `kernel.register_core_plugin(...)` shortcut.

Add a `register_core` call alongside the existing ones, using the
`core_manifest_with_ipc` helper to build the manifest that maps
command names to your numeric handler ids:

```rust
// crates/nexus-bootstrap/src/lib.rs — inside register_core_plugins(…)

use nexus_hello::{HelloPlugin, HANDLER_SAY_HI};

loader
    .register_core(
        core_manifest_with_ipc(
            "com.example.hello",
            "Hello",
            LifecycleFlags { on_init: true, on_start: true, on_stop: true },
            &with_v1_aliases(&[
                ("say_hi", HANDLER_SAY_HI),
            ]),
        ),
        forge_root,
        Box::new(HelloPlugin::new()),
    )
    .or_lifecycle_skip(event_bus, "com.example.hello")?;
```

Notes:

- `LifecycleFlags` tells the loader which of `on_init` / `on_start` /
  `on_stop` your plugin actually overrides. Skip flags for hooks you
  don't implement so the loader doesn't bill you for them.
- `with_v1_aliases(&[("say_hi", HANDLER_SAY_HI), …])` registers every
  command under both the bare name and a `.v1` suffix per ADR 0021
  (handler versioning). The mapping value is your `u32` handler id —
  exactly what `dispatch` matches against.
- `.or_lifecycle_skip(event_bus, "<plugin id>")` is the standard
  failure mode: a plugin that errors during init/start gets skipped
  and the bootstrap continues. Without it, a single plugin failure
  aborts the whole runtime.

Order matters: register dependencies before dependents. The current
`register_core_plugins` function is the canonical record of load order
— [`crates/nexus-bootstrap/src/lib.rs`](../../../crates/nexus-bootstrap/src/lib.rs)
is the source of truth.

## Capabilities

Core plugins declare their capabilities for **observability**, not
enforcement — they're trusted with everything. The capability set
shows up in `nexus plugin list` and the UI so users know what each
plugin claims to do.

```rust
capabilities: CapabilitySet::from_iter([
    Capability::FsRead,
    Capability::FsWrite,
    Capability::EventsPublish,
])
```

Don't list capabilities you don't actually use; the audit value
depends on accuracy.

## IPC: handle vs. call

Handle inbound calls in `dispatch` (or `dispatch_async` for futures).
To call **out** to another plugin, use the `PluginContext` trait method
your `wire_context` stashed:

```rust
use std::time::Duration;

let ctx = self.ctx.as_ref().expect("wire_context not called");
let notes = ctx
    .ipc_call(
        "com.nexus.storage",
        "list_notes",
        serde_json::json!({ "prefix": "projects/" }),
        Duration::from_secs(5),
    )
    .await?;
```

`ipc_call` is `async` and **requires a `timeout: Duration`** — there is
no untimed variant by design. Errors come back as `IpcError`
(`PluginNotFound`, `CommandNotFound`, `Timeout`, `PluginCrashedDuringCall`,
`CapabilityDenied`). Same dispatch path as the TypeScript API; there
is no shortcut from one core plugin to another that bypasses the
kernel.

## Events

The `PluginContext` trait exposes `publish` (sync — kernel populates
metadata) and `subscribe` (returns a dropped-on-go-out-of-scope
`EventSubscription`):

```rust
use nexus_kernel::event_bus::EventFilter;

let ctx = self.ctx.clone().expect("wire_context not called");

// Publish — type_id must start with the plugin's own reverse-DNS namespace.
ctx.publish(
    "com.example.hello.greeted",
    serde_json::json!({ "name": "World" }),
)?;

// Subscribe — keep the returned subscription alive (e.g. on `&mut self`)
// or it auto-unsubscribes. The handler runs on the bus dispatch task; spawn
// a tokio task for any blocking work.
let sub = ctx.subscribe(
    EventFilter::by_type_id("com.nexus.storage.file_changed".to_string()),
);
```

`publish` enforces the namespace rule (`type_id` must start with the
plugin's own id) so plugins can't spoof events from each other. The
kernel marshals events between Rust core plugins and TypeScript /
WASM community plugins automatically.

## Tests

Per-plugin unit tests live alongside the source. Integration tests
that exercise IPC dispatch live in `crates/nexus-bootstrap/tests/` and
build a real runtime in a `tempfile`-allocated forge:

```rust
// crates/nexus-bootstrap/tests/hello_ipc.rs
use std::time::Duration;
use nexus_bootstrap::build_cli_runtime;

#[tokio::test]
async fn say_hi_returns_greeting() {
    let tempdir = tempfile::tempdir().unwrap();
    let runtime = build_cli_runtime(tempdir.path().to_path_buf()).unwrap();

    let result = runtime
        .context
        .ipc_call(
            "com.example.hello",
            "say_hi",
            serde_json::json!("World"),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

    assert_eq!(result["greeting"], "Hello, World!");
}
```

The other `crates/nexus-bootstrap/tests/*.rs` files use the
`common::MinimalForge` helper (see `tests/common/mod.rs`) which wraps
the `tempdir + build_cli_runtime` setup and exposes a
`forge.ipc_call(plugin, command, args)` shorthand with a standard
timeout — prefer that for new tests in the same crate.

Building a runtime gives you a fully-wired kernel with every core
plugin registered, including yours.

## Architectural rules

The hard rules for core plugin authors (enforced by
`crates/nexus-bootstrap/tests/dep_invariants.rs`):

1. **Your plugin crate must depend on `nexus-kernel`, not the other
   way around.** Routing the kernel through your subsystem is the
   error mode.
2. **No direct deps on other service crates.** If `nexus-hello`
   needs storage, it calls through `ipc_call("com.nexus.storage",
   …)`. The dependency invariants test fails the build if you add a
   `nexus-storage` line to your `Cargo.toml`.
3. **Keep state in your plugin.** The kernel doesn't know what your
   plugin does. Pass everything you need through `PluginContext`.

These rules are why the kernel stays small and stable. They feel
restrictive when you're writing a plugin and tempting to break;
breaking them is what kills microkernels.

ADRs:
[`../../adr/0016-microkernel-native-vs-wasm-plugin-split.md`](../../adr/0016-microkernel-native-vs-wasm-plugin-split.md),
[`../../architecture/invariants.md`](../../architecture/invariants.md).

## IPC schema generation

If your plugin's IPC commands take or return structured types,
derive `serde::Serialize`, `serde::Deserialize`, and `schemars::JsonSchema`
on them, then add them to the IPC schema generator. The drift check
will pin them:

```bash
scripts/check_ipc_drift.sh
```

This regenerates TS bindings (`packages/nexus-extension-api/src/generated/ipc/`)
and JSON Schema (`crates/nexus-bootstrap/schemas/ipc/`) so community
plugins get types automatically. See
[`../../ipc-schemas.md`](../../ipc-schemas.md).

## Templates

A scaffold for a core plugin lives at:

```
docs/PRDs/templates/core-plugin/
```

Copy it to `crates/nexus-<name>/` and edit. The template includes the
trait skeleton, a Cargo.toml with the minimum dependency set, and a
unit test that exercises `info()`.

## See also

- [Architecture primer](../architecture-primer.md)
- [`../../architecture/invariants.md`](../../architecture/invariants.md)
- [`../../shell/core-plugins.md`](../../shell/core-plugins.md)
  — catalog of existing core plugins and load order.
