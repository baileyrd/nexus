//! BL-092 — IPC dispatch criterion benchmarks.
//!
//! These exercise the kernel-context → SharedPluginLoader dispatch
//! path that every plugin-to-plugin call (and every CLI / TUI / shell
//! invocation) routes through. We measure pure dispatch overhead with
//! a noop core handler so the numbers reflect the kernel's
//! responsibility, not the handler's.

use std::sync::Arc;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};
use nexus_kernel::{
    Capability, CapabilitySet, EventBus, InMemoryKvStore, KernelPluginContext, KvStore,
    PluginContext,
};
use nexus_plugins::{
    parse_manifest, CorePlugin, PluginError, PluginLoader, SharedPluginLoader,
};

const TIMEOUT: Duration = Duration::from_secs(1);

struct NoopHandler;

impl CorePlugin for NoopHandler {
    fn dispatch(
        &mut self,
        _handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Ok(serde_json::Value::Null)
    }
}

fn core_manifest_with_ipc(
    id: &str,
    cmd: &str,
    handler_id: u32,
) -> nexus_plugins::PluginManifest {
    let toml = format!(
        r#"
[plugin]
id = "{id}"
name = "Bench"
version = "1.0.0"
trust_level = "core"
api_version = "1"

[[registrations.ipc_command]]
id = "{cmd}"
handler_id = {handler_id}
"#
    );
    parse_manifest(&toml, "plugin.toml").expect("parse manifest")
}

fn caller_caps() -> CapabilitySet {
    let mut s = CapabilitySet::empty();
    s.insert(Capability::IpcCall);
    s
}

struct BenchFixture {
    _forge: tempfile::TempDir,
    rt: tokio::runtime::Runtime,
    ctx: Arc<KernelPluginContext>,
}

fn build_fixture() -> BenchFixture {
    let forge = tempfile::tempdir().expect("forge tempdir");
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let plugin_dir = plugins_dir.path();

    let mut loader = PluginLoader::new(plugin_dir);
    loader
        .register_core(
            core_manifest_with_ipc("dev.bench.target", "noop", 1),
            plugin_dir,
            Box::new(NoopHandler),
        )
        .expect("register target");
    let shared = Arc::new(SharedPluginLoader::new(loader));
    let dispatcher: Arc<dyn nexus_kernel::IpcDispatcher> =
        Arc::clone(&shared) as Arc<dyn nexus_kernel::IpcDispatcher>;

    let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::default());
    let event_bus = Arc::new(EventBus::new(64));

    let ctx = Arc::new(
        KernelPluginContext::new(
            "dev.bench.caller",
            "1.0.0",
            caller_caps(),
            kv,
            event_bus,
            forge.path(),
            Some(dispatcher),
        )
        .expect("ctx"),
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");

    BenchFixture { _forge: forge, rt, ctx }
}

fn bench_dispatch_noop_handler(c: &mut Criterion) {
    let fx = build_fixture();
    c.bench_function("ipc_dispatch/noop_handler", |b| {
        b.iter(|| {
            let result = fx.rt.block_on(fx.ctx.ipc_call(
                "dev.bench.target",
                "noop",
                serde_json::Value::Null,
                TIMEOUT,
            ));
            result.expect("dispatch ok");
        });
    });
}

fn bench_capability_check(c: &mut Criterion) {
    // Pure capability lookup against the live shared cap set —
    // covers the steady-state cost of every gate
    // (`PluginContext::has_capability`).
    let fx = build_fixture();
    c.bench_function("ipc_dispatch/capability_check", |b| {
        b.iter(|| {
            let _ = fx.ctx.has_capability(Capability::IpcCall);
        });
    });
}

fn bench_dispatch_ten_concurrent(c: &mut Criterion) {
    // Synchronous serial dispatch ten times in a row. Approximates
    // queue depth: the per-target backend mutex serialises calls, so
    // ten calls = ten lock-acquire cycles on top of the dispatch
    // overhead.
    let fx = build_fixture();
    c.bench_function("ipc_dispatch/serial_ten", |b| {
        b.iter(|| {
            for _ in 0..10 {
                let _ = fx
                    .rt
                    .block_on(fx.ctx.ipc_call(
                        "dev.bench.target",
                        "noop",
                        serde_json::Value::Null,
                        TIMEOUT,
                    ))
                    .expect("dispatch ok");
            }
        });
    });
}

criterion_group!(
    benches,
    bench_dispatch_noop_handler,
    bench_capability_check,
    bench_dispatch_ten_concurrent,
);
criterion_main!(benches);
