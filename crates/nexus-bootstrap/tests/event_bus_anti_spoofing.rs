//! Bootstrap-level guard for ADR 0007's anti-spoofing invariants.
//!
//! ADR 0007 declares three properties of `NexusEvent`:
//!
//! 1. **Plugins cannot emit kernel-tier events.** Anything routed through
//!    `PluginContext::publish` materialises as a `NexusEvent::Custom` variant;
//!    the other variants (`PluginStarted`, `CapabilityGranted`, ...) are
//!    kernel-owned and unreachable from the public surface.
//!
//! 2. **`type_id` is in the plugin's namespace.** Publishing with a `type_id`
//!    that doesn't fall under the calling plugin's reverse-DNS id is rejected
//!    with `BusError::TypeIdNamespaceMismatch` before the event hits the bus.
//!
//! 3. **`emitting_plugin` is set by the kernel.** The plugin doesn't construct
//!    the event — the kernel does — so the field always matches the caller's
//!    plugin id and cannot be spoofed.
//!
//! Unit tests in `nexus-kernel/src/{context_impl,event_bus}.rs` already cover
//! (2) and (3) at the kernel-internal API level. This file locks the same
//! properties end-to-end through a fully-booted runtime so that future
//! refactors of the bootstrap wiring — register-order changes, alternate
//! event-bus impls, capability rewrites — cannot regress the contract
//! without a CI failure.

#[path = "common/mod.rs"]
mod common;

use common::MinimalForge;

use nexus_bootstrap::CLI_PLUGIN_ID;
use nexus_kernel::{BusError, Error, EventFilter, Events as _, NexusEvent};

/// (Property 1 + 3) Publishing through a real `PluginContext` produces a
/// `NexusEvent::Custom` whose `emitting_plugin` is the calling plugin's id,
/// regardless of the payload shape. There is no public surface that lets a
/// plugin construct any other variant.
#[tokio::test]
async fn ctx_publish_produces_custom_with_kernel_set_emitter() {
    let forge = MinimalForge::new();
    let ctx = &forge.runtime.context;

    let bus = forge.runtime.kernel.event_bus();
    let mut sub = bus.subscribe(EventFilter::CustomExact(format!(
        "{CLI_PLUGIN_ID}.anti_spoofing_probe"
    )));

    // Publish a payload whose JSON shape mimics a kernel-owned event.
    // Even if the caller serialises something that looks like
    // `PluginStarted`, the wire variant is forced to `Custom` by the
    // kernel — the plugin never gets to hand the bus a non-Custom enum.
    let masquerade_payload = serde_json::json!({
        "type": "PluginStarted",
        "plugin_id": "com.evil.spoofer",
    });
    ctx.publish(
        &format!("{CLI_PLUGIN_ID}.anti_spoofing_probe"),
        masquerade_payload.clone(),
    )
    .expect("publish in own namespace succeeds");

    let published = sub
        .recv()
        .await
        .expect("subscriber receives the published event");

    // Property 1: the event is `Custom`, not the spoofed kernel-tier variant.
    let NexusEvent::Custom {
        type_id,
        emitting_plugin,
        payload,
    } = &published.event
    else {
        panic!(
            "ADR 0007 invariant broken: plugin publish produced \
             non-Custom variant {:?}",
            published.event
        );
    };

    // Property 3: kernel set `emitting_plugin` from the calling context;
    // the caller had no way to override it.
    assert_eq!(
        emitting_plugin, CLI_PLUGIN_ID,
        "emitting_plugin must reflect the calling plugin's id, not the payload"
    );
    assert_eq!(type_id, &format!("{CLI_PLUGIN_ID}.anti_spoofing_probe"));
    assert_eq!(payload, &masquerade_payload);

    // And the metadata source_plugin_id has the same property —
    // setting it from the context is what makes (3) hold for routing.
    assert_eq!(published.metadata.source_plugin_id, CLI_PLUGIN_ID);
}

/// (Property 2) A namespace-mismatched `type_id` is rejected with
/// `BusError::TypeIdNamespaceMismatch` before reaching the bus. Subscribers
/// see nothing.
#[tokio::test]
async fn ctx_publish_rejects_foreign_namespace() {
    let forge = MinimalForge::new();
    let ctx = &forge.runtime.context;

    let bus = forge.runtime.kernel.event_bus();
    let mut sub = bus.subscribe(EventFilter::CustomPrefix("com.victim.".to_string()));

    let foreign_topic = "com.victim.private_signal";
    let err = ctx
        .publish(foreign_topic, serde_json::json!({ "spoofed": true }))
        .expect_err("publish outside own namespace must fail");

    match err {
        Error::Bus(BusError::TypeIdNamespaceMismatch { plugin_id, type_id }) => {
            assert_eq!(plugin_id, CLI_PLUGIN_ID);
            assert_eq!(type_id, foreign_topic);
        }
        other => panic!(
            "expected BusError::TypeIdNamespaceMismatch, got {other:?} — \
             ADR 0007 property 2 has regressed"
        ),
    }

    // Subscriber never saw the event — anti-spoofing is enforced before
    // the broadcast send, not after.
    assert!(
        sub.try_recv()
            .expect("try_recv shouldn't error on empty bus")
            .is_none(),
        "rejected publish must not reach subscribers"
    );
}

/// (Property 2 — substring guard) A plugin id is a true dot-separated
/// namespace, not a string prefix. `com.nexus.cli-evil.foo` must NOT be
/// accepted just because it `starts_with("com.nexus.cli")`.
///
/// This is the specific class of spoof that `type_id_in_namespace` exists
/// to defend against — the kernel-internal unit test
/// (`publish_rejects_substring_prefix_spoof`) covers the helper directly;
/// this case proves the guard still fires when reached through the real
/// `PluginContext::publish` boundary.
#[tokio::test]
async fn ctx_publish_rejects_prefix_substring_spoof() {
    let forge = MinimalForge::new();
    let ctx = &forge.runtime.context;

    // `com.nexus.cli-evil.foo` shares the prefix of CLI_PLUGIN_ID
    // ("com.nexus.cli") but is NOT inside its dot-separated namespace.
    let near_miss = format!("{CLI_PLUGIN_ID}-evil.foo");
    let err = ctx
        .publish(&near_miss, serde_json::json!({}))
        .expect_err("substring-prefix publish must fail");

    assert!(
        matches!(err, Error::Bus(BusError::TypeIdNamespaceMismatch { .. })),
        "expected TypeIdNamespaceMismatch for substring-spoof, got {err:?}"
    );
}
