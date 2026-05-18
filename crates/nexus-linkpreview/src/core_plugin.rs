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
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{fetch_blocking, FetchError};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.linkpreview";

/// `fetch` handler id.
pub const HANDLER_FETCH: u32 = 1;

/// Args for `com.nexus.linkpreview::fetch` (handler id `1`).
///
/// Lifted to a file-scope public type by audit-2026-05-01 P1-3 (#113)
/// so the schema generator can emit a JSON Schema + TypeScript binding
/// for the IPC contract. Previously inlined inside [`dispatch_fetch`];
/// behaviour is unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct FetchArgs {
    /// URL to fetch. Must be `http`/`https`; the engine rejects other
    /// schemes with [`FetchError::InvalidUrl`].
    pub url: String,
}

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
    let a: FetchArgs = serde_json::from_value(args.clone())
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
                url: a.url,
                ..Default::default()
            };
            serde_json::to_value(&fallback)
                .map_err(|e| exec_err(format!("fetch: serialize fallback: {e}")))
        }
    }
}

nexus_plugins::define_dispatch_helpers!();
