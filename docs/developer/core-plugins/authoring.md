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

Implement the `CorePlugin` trait from `nexus-plugin-api`:

```rust
use async_trait::async_trait;
use nexus_plugin_api::{CorePlugin, PluginContext, PluginInfo, IpcError};
use serde_json::Value;

pub struct HelloPlugin {
    /* internal state */
}

#[async_trait]
impl CorePlugin for HelloPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "com.example.hello".to_string(),
            name: "Hello".to_string(),
            version: "1.0.0".to_string(),
            trust_level: TrustLevel::Core,
            status: PluginStatus::Loaded,
            capabilities: CapabilitySet::from_iter([Capability::UiNotify]),
        }
    }

    async fn on_init(&mut self, ctx: &PluginContext) -> Result<(), String> {
        // One-time setup. State you need before commands can run.
        Ok(())
    }

    async fn on_start(&mut self, ctx: &PluginContext) -> Result<(), String> {
        // Subscribe to events, kick off background tasks.
        Ok(())
    }

    async fn on_stop(&mut self, ctx: &PluginContext) -> Result<(), String> {
        // Graceful shutdown — flush state, cancel tasks.
        Ok(())
    }

    async fn handle_ipc(
        &self,
        command: &str,
        args: Value,
        ctx: &PluginContext,
    ) -> Result<Value, IpcError> {
        match command {
            "say_hi" => {
                let name: String = serde_json::from_value(args)
                    .map_err(|e| IpcError::InvalidArgs(e.to_string()))?;
                Ok(serde_json::json!({ "greeting": format!("Hello, {name}!") }))
            }
            other => Err(IpcError::CommandNotFound {
                plugin_id: self.info().id,
                command: other.to_string(),
            }),
        }
    }
}
```

The lifecycle hooks — `on_init`, `on_start`, `on_stop` — map to the
states in [Lifecycle](../plugins/lifecycle.md). Each can be sync or
async; an `Err` halts the plugin (`Status::Crashed`).

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

The bootstrap crate is the single point that wires every core plugin
into the kernel. Add your plugin in `crates/nexus-bootstrap/src/lib.rs`:

```rust
pub fn build_runtime(forge_root: &Path) -> Result<Runtime, BootstrapError> {
    let mut kernel = Kernel::new(/* … */);

    // ...existing core plugins...
    kernel.register_core_plugin(Box::new(nexus_hello::HelloPlugin::default()))?;

    Ok(Runtime { kernel, /* … */ })
}
```

Order matters: register dependencies before dependents. The
bootstrap module is the canonical record of load order — see
[`docs/shell/core-plugins.md`](../../shell/core-plugins.md).

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

Handle inbound calls in `handle_ipc`. To call other plugins:

```rust
let notes: Value = ctx.ipc_call(
    "com.nexus.storage",
    "list_notes",
    serde_json::json!({ "prefix": "projects/" }),
).await?;
```

Same shape as the TypeScript API — and the same dispatch path. There
is no shortcut from one core plugin to another that bypasses the
kernel.

## Events

```rust
ctx.events_subscribe("files:changed", |event| {
    // Sync handler. For async work, spawn a task.
    tokio::spawn(handle_file_change(event));
}).await?;

ctx.events_publish("hello:greeted", &json!({ "name": "World" })).await?;
```

The kernel marshals events between Rust core plugins and TypeScript
community plugins automatically.

## Tests

Per-plugin unit tests live alongside the source. Integration tests
that exercise IPC dispatch live in `crates/nexus-bootstrap/tests/`:

```rust
// crates/nexus-bootstrap/tests/hello_ipc.rs
use nexus_bootstrap::build_test_runtime;

#[tokio::test]
async fn say_hi_returns_greeting() {
    let runtime = build_test_runtime();
    let result = runtime
        .ipc_call("com.example.hello", "say_hi", json!("World"))
        .await
        .unwrap();
    assert_eq!(result["greeting"], "Hello, World!");
}
```

The bootstrap test runtime gives you a fully-wired kernel with every
core plugin registered.

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
