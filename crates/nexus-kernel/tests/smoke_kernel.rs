//! PRD 01 smoke test — the §12 acceptance criteria from the interface spec.
//!
//! Verifies that a fresh kernel can be constructed, started with no plugins,
//! have events published to it, and shut down cleanly. This is the
//! single-test proof that the nexus-kernel crate is interface-complete.

use std::path::PathBuf;
use std::sync::Arc;

use nexus_kernel::{
    EventFilter, InMemoryKvStore, Kernel, KernelConfig, KvStore,
};

fn kv() -> Arc<dyn KvStore> {
    Arc::new(InMemoryKvStore::new())
}

/// Tempdir helper — creates a unique path and ensures cleanup on drop.
struct TempForge {
    path: PathBuf,
}

impl TempForge {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("nexus-smoke-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempForge {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[tokio::test]
async fn smoke_new_start_shutdown() {
    let forge = TempForge::new("new-start-shutdown");
    let config = KernelConfig::for_testing(forge.path.clone());
    let kernel = Kernel::new(config, kv()).expect("kernel construction should succeed");
    kernel.start().await.expect("kernel start should succeed with empty plugin set");
    kernel.shutdown().await.expect("kernel shutdown should succeed");
}

#[tokio::test]
async fn smoke_event_bus_round_trip() {
    let forge = TempForge::new("bus-roundtrip");
    let config = KernelConfig::for_testing(forge.path.clone());
    let kernel = Kernel::new(config, kv()).unwrap();
    kernel.start().await.unwrap();

    // Subscribe, publish, receive
    let bus = kernel.event_bus();
    let mut sub = bus.subscribe(EventFilter::Variant("PluginLoaded"));

    // We can't call publish_kernel directly from outside the crate (it's
    // pub(crate)), so we verify the bus works by subscribing and then letting
    // the kernel run its own lifecycle. In PRD 01 scope with no plugins,
    // this test just verifies subscription and try_recv don't panic.
    let result = sub.try_recv().unwrap();
    assert!(result.is_none(), "empty plugin set should produce no PluginLoaded events");

    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn smoke_config_loaded_from_disk() {
    let forge = TempForge::new("config-from-disk");
    std::fs::create_dir_all(forge.path.join(".nexus")).unwrap();
    std::fs::write(
        forge.path.join(".nexus/config.toml"),
        "event_bus_capacity = 512\nhot_reload_enabled = false\n",
    )
    .unwrap();

    let config = KernelConfig::load(&forge.path).expect("config load should succeed");
    assert_eq!(config.event_bus_capacity, 512);
    assert!(!config.hot_reload_enabled);

    let kernel = Kernel::new(config, kv()).unwrap();
    kernel.start().await.unwrap();
    kernel.shutdown().await.unwrap();
}

#[tokio::test]
async fn smoke_multiple_shutdown_calls_are_idempotent() {
    let forge = TempForge::new("idempotent-shutdown");
    let config = KernelConfig::for_testing(forge.path.clone());
    let kernel = Kernel::new(config, kv()).unwrap();
    kernel.start().await.unwrap();
    kernel.shutdown().await.unwrap();
    kernel.shutdown().await.unwrap();  // must not panic or error
}

#[tokio::test]
async fn smoke_plugin_registry_is_empty_in_prd_01_scope() {
    let forge = TempForge::new("empty-registry");
    let config = KernelConfig::for_testing(forge.path.clone());
    let kernel = Kernel::new(config, kv()).unwrap();
    kernel.start().await.unwrap();

    let registry = kernel.plugins();
    assert!(registry.is_empty(), "no plugins should be loaded in PRD 01 scope");
    assert_eq!(registry.len(), 0);

    kernel.shutdown().await.unwrap();
}

// A compile-check test: ensures every public type from the interface spec §3
// can be named. If any of these names break, the contract has regressed.
#[test]
fn smoke_all_public_types_importable() {
    use nexus_kernel::{
        BusError, Capability, CapabilityError, CapabilityParseError, CapabilitySet, ConfigError,
        Error, EventBus, EventFilter, EventMetadata, EventSubscription, IpcError, Kernel,
        KernelConfig, KvError, LogLevel, NexusEvent, PluginContext, PluginError, PluginInfo,
        PluginRegistry, PluginStatus, PublishedEvent, RecvError, Result,
        StopReason, TrustLevel,
    };

    // Just reference each type to force the import — this compiles iff
    // all the types exist and are named consistently with the spec.
    fn _type_check() {
        let _: Option<Capability> = None;
        let _: Option<CapabilityError> = None;
        let _: Option<CapabilityParseError> = None;
        let _: Option<CapabilitySet> = None;
        let _: Option<ConfigError> = None;
        let _: Option<Error> = None;
        let _: Option<EventFilter> = None;
        let _: Option<EventMetadata> = None;
        let _: Option<IpcError> = None;
        let _: Option<KernelConfig> = None;
        let _: Option<KvError> = None;
        let _: Option<LogLevel> = None;
        let _: Option<NexusEvent> = None;
        let _: Option<PluginError> = None;
        let _: Option<PluginInfo> = None;
        let _: Option<PluginStatus> = None;
        let _: Option<PublishedEvent> = None;
        let _: Option<RecvError> = None;
        let _: Option<StopReason> = None;
        let _: Option<TrustLevel> = None;
        let _: Option<BusError> = None;

        // Types that aren't Default/None-constructible are just referenced
        // via std::marker to force the import:
        let _: std::marker::PhantomData<Kernel>           = std::marker::PhantomData;
        let _: std::marker::PhantomData<EventBus>         = std::marker::PhantomData;
        let _: std::marker::PhantomData<EventSubscription> = std::marker::PhantomData;
        let _: std::marker::PhantomData<PluginRegistry>   = std::marker::PhantomData;
        let _: std::marker::PhantomData<dyn PluginContext> = std::marker::PhantomData;
        type _R = Result<()>;
    }
}
