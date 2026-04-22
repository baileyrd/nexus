//! Core plugin wrapping [`fetch_blocking`].
//!
//! Exposes a single IPC handler — `fetch` — so the shell's canvas
//! link-node overlay can ask the kernel for preview metadata without
//! linking `nexus-linkpreview` (or reqwest) directly.
//!
//! # Handlers
//!
//! | Id | Command | Args              | Returns                           |
//! |---:|---------|-------------------|-----------------------------------|
//! | 1  | `fetch` | `{ url: String }` | [`super::LinkPreview`] as JSON    |
//!
//! Ids are append-only.

use nexus_plugins::{CorePlugin, PluginError};
use serde::Deserialize;

use crate::{fetch_blocking, FetchError};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.linkpreview";

/// `fetch` handler id.
pub const HANDLER_FETCH: u32 = 1;

/// Core plugin — stateless; every call hits the network fresh. The
/// shell layer owns caching so previews survive across tab switches
/// without paying a second request.
#[derive(Default)]
pub struct LinkPreviewCorePlugin;

impl LinkPreviewCorePlugin {
    /// Construct a fresh plugin. Cheap — no I/O.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl CorePlugin for LinkPreviewCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_FETCH => dispatch_fetch(args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

fn dispatch_fetch(args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
    #[derive(Deserialize)]
    struct Args {
        url: String,
    }
    let a: Args = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("fetch: invalid args: {e}")))?;
    match fetch_blocking(&a.url) {
        Ok(preview) => serde_json::to_value(&preview)
            .map_err(|e| exec_err(format!("fetch: serialize: {e}"))),
        // Fetch errors are expected (bad URLs, offline hosts, 4xx/5xx).
        // Map them to a fallback preview so the shell can render *something*
        // rather than surfacing a raw IPC error for every missed link.
        Err(FetchError::InvalidUrl(url)) => {
            Err(exec_err(format!("invalid URL: {url}")))
        }
        Err(err) => {
            tracing::debug!(%err, "link preview fetch failed; returning empty preview");
            let fallback = crate::LinkPreview {
                url: serde_json::from_value::<Args>(args.clone())
                    .map(|a| a.url)
                    .unwrap_or_default(),
                ..Default::default()
            };
            serde_json::to_value(&fallback)
                .map_err(|e| exec_err(format!("fetch: serialize fallback: {e}")))
        }
    }
}

fn exec_err(msg: impl Into<String>) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: msg.into(),
    }
}
