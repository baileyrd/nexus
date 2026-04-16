//! URI / deep-link dispatch bridge.
//!
//! Provides the `dispatch_uri` Tauri command that the backend (or a future
//! `tauri-plugin-deep-link` integration) calls when the OS delivers an
//! incoming URL. The command emits a `nexus:url-opened` Tauri event with the
//! raw URL string; the frontend `App` component listens and forwards to the
//! contribution-registry `dispatchUri` which routes to every registered
//! `UriHandler` matching the URL's scheme.
//!
//! OS-level scheme registration (writing to the Windows registry,
//! macOS `Info.plist`, or Linux `.desktop` file) is the responsibility of
//! `tauri-plugin-deep-link` and is intentionally out of scope here.

#![allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)]

use tauri::{AppHandle, Emitter};

/// Tauri event name consumed by the frontend URI-handler dispatch loop.
const URL_OPENED_EVENT: &str = "nexus:url-opened";

/// Dispatch an incoming URL to all registered frontend URI handlers.
///
/// Emits `nexus:url-opened` with the raw URL string as the payload.
/// The frontend `App` component listens for this event and calls
/// `contributions.dispatchUri(url)`, which routes to every `UriHandler`
/// registered for the URL's scheme.
///
/// # Usage
///
/// Call this command from any Rust code that receives an incoming URL —
/// e.g. a `tauri-plugin-deep-link` callback:
/// ```rust,ignore
/// use tauri_plugin_deep_link::DeepLinkExt;
/// app_handle.deep_link().on_open_url(|event| {
///     for url in event.urls() {
///         let _ = dispatch_uri_impl(&app_handle, url.as_str());
///     }
/// });
/// ```
#[tauri::command]
pub fn dispatch_uri(url: String, app: AppHandle) -> Result<(), String> {
    dispatch_uri_impl(&app, &url)
}

pub(crate) fn dispatch_uri_impl(app: &AppHandle, url: &str) -> Result<(), String> {
    app.emit(URL_OPENED_EVENT, url)
        .map_err(|e| format!("dispatch_uri: emit failed: {e}"))
}
