//! C85 (#438) — a generic developer-facing tail over the kernel event
//! bus. `nexus watch` only ever subscribed to `com.nexus.storage.*` and
//! formatted just the four file topics; there was no way to observe
//! plugin lifecycle events, capability grants/denials, or another
//! plugin's custom events from the CLI.

use anyhow::Result;
use nexus_kernel::{EventFilter, Events as _, NexusEvent};

use crate::app::App;

/// Parse a `nexus events tail --filter` value into an [`EventFilter`].
///
/// Mirrors the manifest `event_subscriber` filter convention already used
/// by plugin loading (`nexus-plugins::loader::parse_event_filter`), so the
/// same filter strings a plugin author writes in a manifest work here too:
///
/// - `""` or `"*"` — every event ([`EventFilter::All`])
/// - a bare kernel variant name (`"PluginLoaded"`, `"CapabilityDenied"`, …)
///   — [`EventFilter::Variant`]
/// - a `"prefix.*"` string — [`EventFilter::CustomPrefix`] (keeps the
///   trailing `.`)
/// - anything else — [`EventFilter::CustomExact`] on a `Custom` `type_id`
fn parse_filter(filter: &str) -> EventFilter {
    match filter {
        "" | "*" => EventFilter::All,
        "PluginLoaded" | "PluginStarted" | "PluginStopped" | "PluginCrashed"
        | "CapabilityGranted" | "CapabilityDenied" => EventFilter::Variant(filter.to_string()),
        f if f.ends_with(".*") => EventFilter::CustomPrefix(f[..f.len() - 1].to_string()),
        f => EventFilter::CustomExact(f.to_string()),
    }
}

/// `nexus events tail` — subscribe to the kernel event bus and print each
/// matching event until Ctrl+C. Unlike `nexus watch`, this surfaces every
/// event kind (plugin lifecycle, capability grants/denials, and any
/// plugin's `Custom` events), not just storage file changes.
pub fn tail(app: &mut App, filter: &str) -> Result<()> {
    let event_filter = parse_filter(filter);
    let (runtime, rt) = app.runtime()?;
    let mut sub = runtime.context.subscribe(event_filter);

    println!("Tailing events (filter: {filter}). Press Ctrl+C to stop.");

    rt.block_on(async {
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                maybe = sub.recv() => {
                    match maybe {
                        Ok(evt) => print_event(&evt.metadata, &evt.event),
                        Err(_) => break,
                    }
                }
            }
        }
    });

    println!("Stopped.");
    Ok(())
}

fn print_event(metadata: &nexus_kernel::EventMetadata, event: &NexusEvent) {
    let ts = metadata.timestamp.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let body = serde_json::to_string(event).unwrap_or_else(|_| "<unserializable>".to_string());
    println!("{ts}  {}  {body}", metadata.source_plugin_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_star_map_to_all() {
        assert!(matches!(parse_filter(""), EventFilter::All));
        assert!(matches!(parse_filter("*"), EventFilter::All));
    }

    #[test]
    fn known_variant_names_map_to_variant_filter() {
        for name in [
            "PluginLoaded",
            "PluginStarted",
            "PluginStopped",
            "PluginCrashed",
            "CapabilityGranted",
            "CapabilityDenied",
        ] {
            match parse_filter(name) {
                EventFilter::Variant(v) => assert_eq!(v, name),
                other => panic!("expected Variant filter for {name}, got {other:?}"),
            }
        }
    }

    #[test]
    fn trailing_star_maps_to_custom_prefix_keeping_the_dot() {
        match parse_filter("com.nexus.storage.*") {
            EventFilter::CustomPrefix(p) => assert_eq!(p, "com.nexus.storage."),
            other => panic!("expected CustomPrefix, got {other:?}"),
        }
    }

    #[test]
    fn anything_else_maps_to_custom_exact() {
        match parse_filter("com.nexus.storage.file_created") {
            EventFilter::CustomExact(t) => assert_eq!(t, "com.nexus.storage.file_created"),
            other => panic!("expected CustomExact, got {other:?}"),
        }
    }
}
