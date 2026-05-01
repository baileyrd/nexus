//! BL-029 — Tauri commands for the multi-window / detachable-panel
//! workspace surface. PRD-17 §1 calls for "multi-window support: main
//! editor window + detachable panels (AI assistant, forge explorer,
//! settings)". This module ships the Rust-side primitives:
//!
//! * [`popout_window`] — open a child `WebviewWindow` loading the same
//!   shell with a `?popout=<id>` query param. The frontend reads that
//!   param and renders a popout-mode shell that hosts a single leaf
//!   instead of the whole workspace tree.
//! * [`close_popout_window`] — close a popout by label. Idempotent.
//! * [`list_popout_windows`] — enumerate live popout window labels +
//!   bounds, used by the workspace store for crash-recovery hydration.
//! * [`get_popout_window_bounds`] / [`set_popout_window_bounds`] —
//!   read/write current size + position so the workspace can persist
//!   them in `<vault>/.forge/workspace.json`.
//!
//! Window labels are namespaced (`POPOUT_LABEL_PREFIX` = "popout-")
//! so the rest of Tauri's window-management surface (the main
//! `"main"` window, `tauri::Manager::webview_windows` listings) stays
//! cleanly partitioned.
//!
//! Each popout window shares the same `Tauri::State<KernelRuntime>`
//! managed by [`crate::bridge`], so `kernel_invoke` etc. work
//! identically from a popout's webview without per-window kernel
//! re-boot.
//!
//! NOTE on popout boot path: the popout webview loads the same
//! `index.html`. The frontend's `main.tsx` checks
//! `new URLSearchParams(window.location.search).get('popout')` and,
//! when set, mounts a stripped-down shell that targets only the
//! requested leaf. The full popout-side leaf rendering is staged in a
//! follow-up — Phase 1 here lands the window-management primitives,
//! the workspace-store API, and the persistence schema. See BL-029
//! note in BACKLOG.md.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Prefix used for every popout window's Tauri label. The launcher
/// window is `"main"` (set in `tauri.conf.json`); we deliberately
/// keep popouts under a prefix so list / filter logic can spot them
/// at a glance without an opt-in registry.
pub const POPOUT_LABEL_PREFIX: &str = "popout-";

/// Frontend page loaded into every popout. The query string carries
/// the popout id + leaf id; the frontend's `main.tsx` reads
/// `window.location.search` to switch into popout-mode rendering.
const POPOUT_ENTRY: &str = "index.html";

/// Default popout window inner size. Matches the rough median of
/// editor + side-panel widths so a fresh popout reads as a usable
/// note window without a manual resize.
const DEFAULT_WIDTH: f64 = 720.0;
const DEFAULT_HEIGHT: f64 = 540.0;
const DEFAULT_MIN_WIDTH: f64 = 320.0;
const DEFAULT_MIN_HEIGHT: f64 = 240.0;

/// Bounds payload for popout windows. Mirrors the `bounds?` field on
/// `FloatingWindow` in `shell/src/workspace/types.ts` (`{ x, y, w, h }`).
/// `x` / `y` are screen-space pixel coordinates; `w` / `h` are inner
/// (client-area) pixel sizes. All four are required when restoring; the
/// frontend supplies sane defaults when the bounds are absent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PopoutBounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Snapshot of a single popout window for `list_popout_windows`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PopoutSnapshot {
    pub label: String,
    pub title: String,
    pub bounds: Option<PopoutBounds>,
}

/// Validate a popout id supplied by the frontend. Ids are user-
/// controlled (workspace-store generates them via `crypto.randomUUID`,
/// but persisted ids may have come from older builds), so refuse
/// anything that contains characters Tauri's window-label rules
/// forbid. The label that's passed to `WebviewWindowBuilder::new` is
/// `<prefix><id>`, and Tauri rejects labels with whitespace or
/// non-ASCII; we additionally forbid `/`, `\`, `?`, `#`, `&` and `=`
/// so the id can't end up smuggling extra query terms or path
/// segments into the URL.
pub fn is_valid_popout_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 128 {
        return false;
    }
    id.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '-' || c == '_'
    })
}

/// Validate a `leaf_id` supplied by the frontend before it gets
/// interpolated into a popout URL's query string. Same character
/// class as [`is_valid_popout_id`] — the audit (#86) flagged that
/// the prior `!leaf.is_empty()` check let a `leaf_id` smuggle extra
/// query parameters (`xxx&malicious=foo`) into the popout webview's
/// URL. Reusing the popout-id rules here keeps the URL-construction
/// invariant uniform: every dynamic segment is `[A-Za-z0-9_-]+`.
#[must_use]
pub fn is_valid_leaf_id(id: &str) -> bool {
    is_valid_popout_id(id)
}

/// Build the `<prefix><id>` window label for a popout id. Caller is
/// expected to have already validated `id` via [`is_valid_popout_id`].
pub fn label_for_id(id: &str) -> String {
    format!("{POPOUT_LABEL_PREFIX}{id}")
}

/// Build the popout-mode URL with `?popout=<id>&leaf=<leafId>` query
/// params. `leaf_id` is optional — a popout can be opened without a
/// pre-attached leaf in case the frontend wants to fill it later.
pub fn build_popout_url(id: &str, leaf_id: Option<&str>) -> String {
    // Pre-#86 the only check on `leaf_id` was `!leaf.is_empty()`; a
    // value like `xxx&malicious=foo` smuggled additional query
    // parameters into the popout webview URL. Now we require the
    // same `[A-Za-z0-9_-]+` shape we require for `id` (validated by
    // [`is_valid_popout_id`] before the URL is built); a leaf_id
    // that fails the check is treated as absent.
    match leaf_id {
        Some(leaf) if is_valid_leaf_id(leaf) => {
            format!("{POPOUT_ENTRY}?popout={id}&leaf={leaf}")
        }
        _ => format!("{POPOUT_ENTRY}?popout={id}"),
    }
}

/// Open a popout `WebviewWindow`. Returns the assigned label.
///
/// `id` is the workspace-store FloatingWindow id. `leaf_id` is the
/// leaf currently attached to that floating window (may be empty if
/// the popout is opened blank). `title` is shown in the OS title bar
/// — the popout has decorations on by default so the user has a
/// native close affordance when the in-page chrome (which is
/// minimal in popout-mode) doesn't suffice.
///
/// Errors:
/// * `"invalid popout id"` — id failed [`is_valid_popout_id`].
/// * `"popout already open"` — a window with this label is already in the
///   webview-window map (idempotency check, not a Tauri-side error).
/// * Otherwise propagates the underlying `WebviewWindowBuilder::build`
///   error stringified.
#[tauri::command]
pub async fn popout_window(
    app: AppHandle,
    id: String,
    leaf_id: Option<String>,
    title: Option<String>,
    bounds: Option<PopoutBounds>,
) -> Result<String, String> {
    if !is_valid_popout_id(&id) {
        return Err("invalid popout id".to_string());
    }
    let label = label_for_id(&id);
    if app.webview_windows().contains_key(&label) {
        return Err("popout already open".to_string());
    }

    let url = build_popout_url(&id, leaf_id.as_deref());
    let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .title(title.unwrap_or_else(|| "Nexus — Popout".to_string()))
        .min_inner_size(DEFAULT_MIN_WIDTH, DEFAULT_MIN_HEIGHT)
        .resizable(true)
        // Popouts get native decorations: the main window hides them in
        // favour of the in-shell `WindowControls`, but a popout's chrome
        // is intentionally minimal so the OS-provided close button is
        // the user's escape hatch.
        .decorations(true);

    if let Some(b) = bounds {
        let w = if b.w >= DEFAULT_MIN_WIDTH { b.w } else { DEFAULT_WIDTH };
        let h = if b.h >= DEFAULT_MIN_HEIGHT { b.h } else { DEFAULT_HEIGHT };
        builder = builder.inner_size(w, h).position(b.x, b.y);
    } else {
        builder = builder.inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT);
    }

    builder
        .build()
        .map_err(|e| format!("popout build failed: {e}"))?;

    Ok(label)
}

/// Close a popout window by id. Idempotent — closing an unknown id
/// returns `Ok(())` so racing closes (user clicks OS-X concurrently
/// with the workspace store firing `close_popout_window`) don't
/// surface as a frontend error.
#[tauri::command]
pub async fn close_popout_window(app: AppHandle, id: String) -> Result<(), String> {
    if !is_valid_popout_id(&id) {
        return Err("invalid popout id".to_string());
    }
    let label = label_for_id(&id);
    let Some(window) = app.get_webview_window(&label) else {
        return Ok(());
    };
    window.close().map_err(|e| format!("close failed: {e}"))
}

/// Return a snapshot of every popout window currently open. Used by
/// the workspace store on boot to reconcile its serialized
/// `floating[]` state against the live OS windows — for the rare case
/// where the previous shell session was force-killed and OS windows
/// outlived our state file (or vice versa).
#[tauri::command]
pub async fn list_popout_windows(app: AppHandle) -> Vec<PopoutSnapshot> {
    let mut out = Vec::new();
    for (label, window) in app.webview_windows().iter() {
        if !label.starts_with(POPOUT_LABEL_PREFIX) {
            continue;
        }
        let bounds = match (window.outer_position(), window.inner_size()) {
            (Ok(pos), Ok(size)) => Some(PopoutBounds {
                x: pos.x as f64,
                y: pos.y as f64,
                w: size.width as f64,
                h: size.height as f64,
            }),
            _ => None,
        };
        let title = window.title().unwrap_or_default();
        out.push(PopoutSnapshot {
            label: label.clone(),
            title,
            bounds,
        });
    }
    out
}

/// Fetch the current bounds of a single popout. Returns `None` when
/// the popout doesn't exist (window has already been closed) so the
/// frontend can treat it as a benign no-op.
#[tauri::command]
pub async fn get_popout_window_bounds(
    app: AppHandle,
    id: String,
) -> Result<Option<PopoutBounds>, String> {
    if !is_valid_popout_id(&id) {
        return Err("invalid popout id".to_string());
    }
    let label = label_for_id(&id);
    let Some(window) = app.get_webview_window(&label) else {
        return Ok(None);
    };
    let pos = window
        .outer_position()
        .map_err(|e| format!("outer_position failed: {e}"))?;
    let size = window
        .inner_size()
        .map_err(|e| format!("inner_size failed: {e}"))?;
    Ok(Some(PopoutBounds {
        x: pos.x as f64,
        y: pos.y as f64,
        w: size.width as f64,
        h: size.height as f64,
    }))
}

/// Move + resize a popout window. Used to restore persisted bounds
/// on hydrate, or to apply a bounds change from the popout's own
/// workspace-store mirror after a drag. No-ops when the popout
/// doesn't exist.
#[tauri::command]
pub async fn set_popout_window_bounds(
    app: AppHandle,
    id: String,
    bounds: PopoutBounds,
) -> Result<(), String> {
    if !is_valid_popout_id(&id) {
        return Err("invalid popout id".to_string());
    }
    let label = label_for_id(&id);
    let Some(window) = app.get_webview_window(&label) else {
        return Ok(());
    };
    // Floor + clamp at minimum so a runaway tiny bounds payload (e.g.
    // a popout that was minimised when bounds were captured on the
    // sender side) doesn't make the window vanish into a 1×1 corner.
    let w = bounds.w.max(DEFAULT_MIN_WIDTH);
    let h = bounds.h.max(DEFAULT_MIN_HEIGHT);
    window
        .set_size(tauri::LogicalSize::new(w, h))
        .map_err(|e| format!("set_size failed: {e}"))?;
    window
        .set_position(tauri::LogicalPosition::new(bounds.x, bounds.y))
        .map_err(|e| format!("set_position failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_ids_pass() {
        assert!(is_valid_popout_id("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_valid_popout_id("abc_123"));
        assert!(is_valid_popout_id("X"));
    }

    #[test]
    fn invalid_ids_fail() {
        assert!(!is_valid_popout_id(""));
        assert!(!is_valid_popout_id("has space"));
        assert!(!is_valid_popout_id("slash/here"));
        assert!(!is_valid_popout_id("query?param"));
        assert!(!is_valid_popout_id("amp&rsand"));
        assert!(!is_valid_popout_id("equals=sign"));
        assert!(!is_valid_popout_id("emoji-🌶️"));
        // Length cap rejects pathological inputs.
        let long = "a".repeat(129);
        assert!(!is_valid_popout_id(&long));
    }

    #[test]
    fn label_uses_prefix() {
        assert_eq!(label_for_id("abc"), "popout-abc");
        assert!(label_for_id("xyz").starts_with(POPOUT_LABEL_PREFIX));
    }

    #[test]
    fn build_url_with_leaf_id() {
        let url = build_popout_url("fw1", Some("leaf-7"));
        assert_eq!(url, "index.html?popout=fw1&leaf=leaf-7");
    }

    #[test]
    fn build_url_without_leaf_id() {
        let url = build_popout_url("fw2", None);
        assert_eq!(url, "index.html?popout=fw2");
    }

    #[test]
    fn build_url_treats_empty_leaf_as_absent() {
        // An empty `leaf=` would still be a valid query but it carries no
        // information and the frontend's URL parser would have to special-
        // case it. Treat empty as absent at the source.
        let url = build_popout_url("fw3", Some(""));
        assert_eq!(url, "index.html?popout=fw3");
    }

    #[test]
    fn bounds_round_trip_through_serde() {
        let b = PopoutBounds { x: 10.0, y: 20.0, w: 800.0, h: 600.0 };
        let json = serde_json::to_string(&b).unwrap();
        let back: PopoutBounds = serde_json::from_str(&json).unwrap();
        assert_eq!(back.x, 10.0);
        assert_eq!(back.y, 20.0);
        assert_eq!(back.w, 800.0);
        assert_eq!(back.h, 600.0);
    }

    #[test]
    fn snapshot_serializes_with_camel_case() {
        let snap = PopoutSnapshot {
            label: "popout-abc".into(),
            title: "Nexus".into(),
            bounds: None,
        };
        let json = serde_json::to_string(&snap).unwrap();
        // No `bounds` field renamed (Option=None serializes as `null`),
        // but the struct itself has no camelCase mismatch — the test
        // here is a guard against accidental field renames.
        assert!(json.contains("\"label\":\"popout-abc\""));
        assert!(json.contains("\"title\":\"Nexus\""));
        assert!(json.contains("\"bounds\":null"));
    }
}
