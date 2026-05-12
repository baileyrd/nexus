// src-tauri/src/lib.rs

mod bridge;
mod persistence;
mod windows;

use std::fs;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use ed25519_dalek::{Signature, VerifyingKey, Verifier};

/// Tauri event channel used to forward OS deep-link URLs to the frontend.
/// The frontend's bootstrap code listens on this channel and forwards
/// each URL string to `uriHandlerRegistry.dispatch(new URL(url))`. See
/// `shell/src/registry/UriHandlerRegistry.ts` header (WI-13) for the
/// contract. This is the Tauri-side bridge referenced in that header.
const DEEP_LINK_EVENT: &str = "nexus:url-opened";

// ── OI-15: Manifest signature verification ────────────────────────────────────

/// Ed25519 public keys (hex) trusted by this build.
/// Empty until the marketplace CA is established; all signed plugins
/// with unrecognised keys are rejected rather than silently loaded.
static TRUSTED_PUBLIC_KEYS: &[&str] = &[];

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum VerificationStatus {
    /// No plugin.json.sig file present — plugin is unsigned.
    #[default]
    Unsigned,
    /// Signature valid and public key is in TRUSTED_PUBLIC_KEYS.
    Verified,
    /// Signature present and cryptographically valid, but the public
    /// key is not in TRUSTED_PUBLIC_KEYS.  Plugin is rejected.
    UntrustedKey,
    /// plugin.json.sig is present but malformed or the signature does
    /// not verify against plugin.json.  Plugin is rejected.
    InvalidSignature,
}

/// Verify `manifest_bytes` (the raw plugin.json bytes) against an
/// optional `plugin.json.sig` file in `dir_path`.
///
/// Sig file format — JSON object:
///   `{ "publicKey": "<64-hex-char Ed25519 key>",
///      "signature": "<128-hex-char Ed25519 signature>" }`
fn verify_plugin_signature(manifest_bytes: &[u8], dir_path: &std::path::Path) -> VerificationStatus {
    let sig_path = dir_path.join("plugin.json.sig");
    if !sig_path.exists() {
        return VerificationStatus::Unsigned;
    }

    let sig_content = match fs::read_to_string(&sig_path) {
        Ok(c)  => c,
        Err(_) => return VerificationStatus::InvalidSignature,
    };

    #[derive(Deserialize)]
    struct SigFile { #[serde(rename = "publicKey")] public_key: String, signature: String }

    let sig_file: SigFile = match serde_json::from_str(&sig_content) {
        Ok(s)  => s,
        Err(_) => return VerificationStatus::InvalidSignature,
    };

    // Decode hex → bytes
    let key_bytes: Vec<u8> = match hex::decode(&sig_file.public_key) {
        Ok(b) if b.len() == 32 => b,
        _                       => return VerificationStatus::InvalidSignature,
    };
    let sig_bytes: Vec<u8> = match hex::decode(&sig_file.signature) {
        Ok(b) if b.len() == 64 => b,
        _                       => return VerificationStatus::InvalidSignature,
    };

    // Check trusted-key list before doing crypto
    if !TRUSTED_PUBLIC_KEYS.contains(&sig_file.public_key.as_str()) {
        return VerificationStatus::UntrustedKey;
    }

    let verifying_key = match VerifyingKey::from_bytes(key_bytes[..32].try_into().unwrap()) {
        Ok(k)  => k,
        Err(_) => return VerificationStatus::InvalidSignature,
    };
    let signature = match Signature::from_slice(&sig_bytes) {
        Ok(s)  => s,
        Err(_) => return VerificationStatus::InvalidSignature,
    };

    match verifying_key.verify(manifest_bytes, &signature) {
        Ok(_)  => VerificationStatus::Verified,
        Err(_) => VerificationStatus::InvalidSignature,
    }
}

// ── Community plugin manifest ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CommunityPluginManifest {
    pub id:          String,
    pub name:        String,
    pub version:     String,
    pub main:        String,
    #[serde(default = "default_true")]
    pub enabled:     bool,
    pub description: Option<String>,
    pub author:      Option<String>,
    /// Plugin API version the plugin targets. WI-33 / Phase 3a — surfaces
    /// the kernel-side `check_api_version` result to the shell UI. Absent
    /// means "legacy plugin"; the shell logs a warn and loads it anyway
    /// (see `communityPluginLoader.ts`). Mismatched values are rejected
    /// before activation with a typed `PluginApiVersionError`.
    #[serde(default)]
    pub api_version: Option<u32>,
    /// Declared capabilities (WI-31 / Phase 3 §4.1). Raw PascalCase strings
    /// matching the `Capability` ts-rs union (e.g. `["FsRead", "NetHttp"]`).
    /// Forwarded verbatim to the shell so `parseManifestCapabilities` can
    /// filter unknown variants and the capability-prompt plugin can drive
    /// the consent flow. `None` when the manifest omits the field entirely,
    /// distinguishing "legacy plugin" from "declared empty".
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
    // Injected by scan — not present in plugin.json on disk
    #[serde(skip_deserializing, default)]
    pub dir:         String,
    #[serde(skip_deserializing, default)]
    pub manifest_path: String,
    /// OI-15 — result of Ed25519 signature check against plugin.json.sig.
    #[serde(skip_deserializing, default)]
    pub verification_status: VerificationStatus,
}

fn default_true() -> bool { true }

// ── Commands ──────────────────────────────────────────────────────────────────

/// Scan ~/.nexus-shell/plugins/ for community plugin bundles.
/// Each bundle is a sub-directory containing plugin.json + a JS entry point.
/// Creates the directory on first run so users know where to drop plugins.
/// Returns both enabled and disabled manifests — the frontend filters.
#[tauri::command]
fn scan_plugin_directory() -> Vec<CommunityPluginManifest> {
    let plugins_dir = match dirs::home_dir() {
        Some(h) => h.join(".nexus-shell").join("plugins"),
        None    => {
            eprintln!("[scan_plugin_directory] Cannot resolve home dir");
            return vec![];
        }
    };

    if !plugins_dir.exists() {
        if let Err(e) = fs::create_dir_all(&plugins_dir) {
            eprintln!("[scan_plugin_directory] Cannot create plugins dir: {e}");
            return vec![];
        }
    }

    let entries = match fs::read_dir(&plugins_dir) {
        Ok(e)  => e,
        Err(e) => {
            eprintln!("[scan_plugin_directory] Cannot read plugins dir: {e}");
            return vec![];
        }
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let dir_path      = e.path();
            let manifest_path = dir_path.join("plugin.json");

            let content = fs::read_to_string(&manifest_path)
                .map_err(|_| eprintln!("[scan_plugin_directory] No plugin.json in {}", dir_path.display()))
                .ok()?;

            let mut manifest: CommunityPluginManifest = serde_json::from_str(&content)
                .map_err(|err| eprintln!(
                    "[scan_plugin_directory] Bad plugin.json in {}: {err}",
                    dir_path.display()
                ))
                .ok()?;

            // Verify main entry point exists before advertising the plugin
            if !dir_path.join(&manifest.main).exists() {
                eprintln!(
                    "[scan_plugin_directory] main '{}' not found in {}",
                    manifest.main, dir_path.display()
                );
                return None;
            }

            // OI-15 — reject plugins with a bad or untrusted signature
            let status = verify_plugin_signature(content.as_bytes(), &dir_path);
            if matches!(status, VerificationStatus::UntrustedKey | VerificationStatus::InvalidSignature) {
                eprintln!(
                    "[scan_plugin_directory] Rejecting {} — {:?}",
                    dir_path.display(), status
                );
                return None;
            }

            manifest.dir                 = dir_path.to_string_lossy().into_owned();
            manifest.manifest_path       = manifest_path.to_string_lossy().into_owned();
            manifest.verification_status = status;
            Some(manifest)
        })
        .collect()
}

/// True iff `dir` looks like a plausible plugin-directory root —
/// non-empty, not the filesystem root, not a system-managed path
/// (`/etc`, `/proc`, `/sys`, `/dev`, `/usr`, `/bin`, `/sbin`,
/// `/boot`, `/var`). Issue #86 defense-in-depth — the renderer is
/// the same trust domain so this isn't a hard security gate, but
/// catching obvious "the user typed the wrong thing" / "the
/// frontend has a bug" cases is cheap.
fn is_plausible_plugin_root(dir: &str) -> bool {
    if dir.is_empty() {
        return false;
    }
    let path = std::path::Path::new(dir);
    if path == std::path::Path::new("/") {
        return false;
    }
    // Reject Unix system directories at the top level.
    const FORBIDDEN_PREFIXES: &[&str] = &[
        "/etc", "/proc", "/sys", "/dev", "/usr", "/bin", "/sbin", "/boot", "/var",
    ];
    for prefix in FORBIDDEN_PREFIXES {
        if dir == *prefix || dir.starts_with(&format!("{prefix}/")) {
            return false;
        }
    }
    true
}

/// Scan an explicit directory path for community plugins.
/// Used in dev mode to load plugins straight from the repo without copying them.
///
/// Issue #86. The renderer is the same trust domain that runs the
/// shell, so a "compromised renderer can pass `/etc`" attack
/// already has dozens of other commands to choose from. The
/// shape-validation here is defense-in-depth: refuse plainly
/// nonsensical paths (empty, root, system directories) so a buggy
/// frontend writing "the wrong path string" surfaces as an early
/// rejection instead of a confusing manifest-list result. A full
/// "renderer cannot pass arbitrary paths" guarantee needs a
/// sandboxed-renderer redesign tracked under #86.
#[tauri::command]
fn scan_plugin_directory_at(dir: String) -> Vec<CommunityPluginManifest> {
    if !is_plausible_plugin_root(&dir) {
        eprintln!(
            "[scan_plugin_directory_at] refusing implausible plugin root: {dir:?}"
        );
        return vec![];
    }
    let plugins_dir = std::path::Path::new(&dir);

    if !plugins_dir.exists() {
        return vec![];
    }

    let entries = match fs::read_dir(plugins_dir) {
        Ok(e)  => e,
        Err(e) => {
            eprintln!("[scan_plugin_directory_at] Cannot read {dir}: {e}");
            return vec![];
        }
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let dir_path      = e.path();
            let manifest_path = dir_path.join("plugin.json");

            let content = fs::read_to_string(&manifest_path).ok()?;
            let mut manifest: CommunityPluginManifest = serde_json::from_str(&content)
                .map_err(|err| eprintln!(
                    "[scan_plugin_directory_at] Bad plugin.json in {}: {err}",
                    dir_path.display()
                ))
                .ok()?;

            if !dir_path.join(&manifest.main).exists() {
                eprintln!(
                    "[scan_plugin_directory_at] main '{}' not found in {}",
                    manifest.main, dir_path.display()
                );
                return None;
            }

            // OI-15 — reject plugins with a bad or untrusted signature
            let status = verify_plugin_signature(content.as_bytes(), &dir_path);
            if matches!(status, VerificationStatus::UntrustedKey | VerificationStatus::InvalidSignature) {
                eprintln!(
                    "[scan_plugin_directory_at] Rejecting {} — {:?}",
                    dir_path.display(), status
                );
                return None;
            }

            manifest.dir                 = dir_path.to_string_lossy().into_owned();
            manifest.manifest_path       = manifest_path.to_string_lossy().into_owned();
            manifest.verification_status = status;
            Some(manifest)
        })
        .collect()
}

/// Persist the enabled/disabled state to a plugin's plugin.json.
#[tauri::command]
fn set_plugin_enabled(plugin_id: String, enabled: bool) -> Result<(), String> {
    let plugins_dir = dirs::home_dir()
        .ok_or_else(|| "Cannot resolve home dir".to_string())?
        .join(".nexus-shell")
        .join("plugins");

    let entries = fs::read_dir(&plugins_dir)
        .map_err(|e| format!("Cannot read plugins dir: {e}"))?;

    for entry in entries.filter_map(|e| e.ok()) {
        let manifest_path = entry.path().join("plugin.json");
        let Ok(content) = fs::read_to_string(&manifest_path) else { continue };
        let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) else { continue };

        if json.get("id").and_then(|v| v.as_str()) == Some(plugin_id.as_str()) {
            json["enabled"] = serde_json::Value::Bool(enabled);
            let updated = serde_json::to_string_pretty(&json)
                .map_err(|e| format!("Serialize error: {e}"))?;
            fs::write(&manifest_path, updated)
                .map_err(|e| format!("Write error: {e}"))?;
            return Ok(());
        }
    }

    Err(format!("Plugin '{plugin_id}' not found"))
}

// ── Granted capabilities (WI-31 — install-time consent) ─────────────────────
//
// The kernel persists HIGH-risk user consents in `<plugin_dir>/granted_caps.json`,
// keyed to the plugin's version string. On every plugin load the kernel reads
// this file and filters the declared capability set accordingly (see
// `crates/nexus-plugins/src/loader.rs::load_granted_high_risk_caps`).
//
// This shell-internal bridge writes the same file from the consent UI —
// consistent with the `set_plugin_enabled` precedent of bypassing the kernel
// runtime for file-backed state that the kernel only reads at boot. We do
// NOT touch the kernel's in-memory grant state; grants take effect on the
// next plugin load (or the next forge boot, which is where this file is
// normally consulted).
//
// Capability strings on the wire use the dotted kernel form (`"fs.read"`,
// `"process.spawn"`) — that's what `Capability::from_str` accepts. The TS
// side owns the PascalCase↔dotted mapping; this Rust layer is a pure
// pass-through of whatever strings the caller sends.

const GRANTED_CAPS_FILENAME: &str = "granted_caps.json";

/// Persisted shape mirrors the kernel's `GrantedCapsFile` at
/// `crates/nexus-plugins/src/loader.rs:1581`. Not imported from the
/// kernel crate on purpose — the shell doesn't link `nexus-plugins` and
/// this struct is trivially small. If the kernel shape evolves, update
/// both sides.
#[derive(Debug, Serialize, Deserialize, Default)]
struct GrantedCapsFile {
    #[serde(default)]
    version: String,
    #[serde(default)]
    granted: Vec<String>,
}

/// A single plugin's grant entry as seen by the shell UI.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GrantedCapabilityEntry {
    /// Plugin version the grant was pinned to. Empty string when the file
    /// is missing — the TS side treats that as "no prior grant".
    pub version: String,
    /// Dotted capability strings the user has granted (e.g. `"fs.read"`).
    pub capabilities: Vec<String>,
}

fn read_granted_entry(plugin_dir: &std::path::Path) -> GrantedCapabilityEntry {
    let path = plugin_dir.join(GRANTED_CAPS_FILENAME);
    let Ok(contents) = fs::read_to_string(&path) else {
        return GrantedCapabilityEntry { version: String::new(), capabilities: Vec::new() };
    };
    match serde_json::from_str::<GrantedCapsFile>(&contents) {
        Ok(f) => GrantedCapabilityEntry {
            version: f.version,
            capabilities: f.granted,
        },
        Err(_) => GrantedCapabilityEntry { version: String::new(), capabilities: Vec::new() },
    }
}

fn write_granted_entry(
    plugin_dir: &std::path::Path,
    entry: &GrantedCapabilityEntry,
) -> Result<(), String> {
    let path = plugin_dir.join(GRANTED_CAPS_FILENAME);
    let mut file = GrantedCapsFile {
        version: entry.version.clone(),
        granted: entry.capabilities.clone(),
    };
    file.granted.sort();
    file.granted.dedup();
    let json = serde_json::to_string_pretty(&file)
        .map_err(|e| format!("serialize granted_caps.json: {e}"))?;
    // Atomic write: tmp + rename so a crash mid-write can't produce a
    // half-written file the kernel will parse as "deny-all".
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| format!("write tmp: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("rename tmp: {e}"))?;
    Ok(())
}

/// Return the granted-caps snapshot for every plugin the scanner can see.
/// Keyed by plugin id. Plugins with no `granted_caps.json` still appear
/// with `version: ""` and `capabilities: []` so the TS side can treat
/// "unknown prior grant" and "all denied" uniformly.
///
/// Both the user's drop folder and (in dev) the repo-side
/// `shell/src/plugins/community` tree are walked — matching
/// `scan_plugin_directory` / `scan_plugin_directory_at`.
#[tauri::command]
fn get_plugin_granted_capabilities(
    plugin_dirs: std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, GrantedCapabilityEntry> {
    let mut out = std::collections::HashMap::new();
    for (plugin_id, dir) in plugin_dirs {
        let p = std::path::Path::new(&dir);
        out.insert(plugin_id, read_granted_entry(p));
    }
    out
}

/// Overwrite the granted capability set for a single plugin. The entry
/// version is pinned to `version` (should be the manifest version at
/// consent time) — a subsequent version bump will make the kernel reset
/// grants on next load (re-prompt). Passing an empty `capabilities`
/// vector clears all prior grants.
///
/// Capability strings MUST be in the dotted kernel form (`"fs.read"`,
/// `"process.spawn"`, …) — the TS consent plugin does the
/// PascalCase→dotted translation before invoking.
///
/// SECURITY (audit-2026-05-01 P2-1): this command mutates the persisted
/// capability grant. The host validates each capability string against
/// `Capability::from_str` (issue #86 hardening) but performs no
/// additional gate — **the renderer-side consent UI is the trust
/// boundary**. The TS-side `consent` plugin must obtain explicit user
/// approval via the consent flow before invoking this command. Any
/// frontend code path that reaches `invoke("set_plugin_granted_capabilities")`
/// without first surfacing the consent dialog is a security bug.
#[tauri::command]
fn set_plugin_granted_capabilities(
    plugin_dir: String,
    version: String,
    capabilities: Vec<String>,
) -> Result<(), String> {
    let p = std::path::Path::new(&plugin_dir);
    if !p.exists() {
        return Err(format!("plugin_dir does not exist: {plugin_dir}"));
    }
    // Issue #86. Pre-fix this was a pure pass-through — a buggy or
    // malicious frontend could persist `["definitely-fake-cap"]` to
    // disk; the kernel's `load_granted_high_risk_caps` would silently
    // filter unknowns at load time, so the bad string just sat there
    // confusing operators and complicating audit. Validate every
    // entry against the canonical kernel enum at write time so the
    // file is always well-formed.
    for cap in &capabilities {
        if nexus_plugin_api::Capability::from_str(cap).is_err() {
            return Err(format!(
                "set_plugin_granted_capabilities: '{cap}' is not a recognised \
                 capability — refusing to persist garbage to granted_caps.json. \
                 Wire form is the dotted kernel name (e.g. 'fs.read', \
                 'process.spawn')."
            ));
        }
    }
    let entry = GrantedCapabilityEntry { version, capabilities };
    write_granted_entry(p, &entry)
}

/// Unscoped path existence check. tauri-plugin-fs scopes paths to a
/// configured allowlist, which rejects arbitrary user-picked folders
/// before we ever see them. This bypass uses std::path directly so the
/// workspace plugin can verify a persisted root on boot without having
/// to preconfigure every possible folder the user might open.
#[tauri::command]
fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

// Git status now routes through the kernel's git plugin via
// `api.kernel.invoke('com.nexus.git', 'status', {})`. The standalone
// `get_git_status` Tauri command (and the direct `git2` dependency it
// pulled in) was retired in Phase 1 of the shell ↔ kernel bridge
// migration (see docs/shell-kernel-bridge-plan.md).

// Directory listing now routes through the kernel's storage plugin via
// `api.kernel.invoke('com.nexus.storage', 'list_dir', { relpath })`. The
// standalone `read_dir` Tauri command was retired in Phase 1 of the
// shell ↔ kernel bridge migration (see docs/shell-kernel-bridge-plan.md).

// ── Renderer log bridge ───────────────────────────────────────────────────────

/// A single log entry forwarded from the browser-side `clientLogger` ring
/// buffer. The `ts` field is a Unix-epoch millisecond timestamp (`Date.now()`);
/// `level` is one of `"debug" | "info" | "warn" | "error"`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererLogEntry {
    pub ts:      u64,
    pub level:   String,
    pub message: String,
}

/// Receive a batch of log entries from the browser-side `clientLogger` and
/// forward them through the Rust `tracing` subscriber with the
/// `nexus_shell::renderer` target so they appear alongside kernel logs.
///
/// Called by `clientLogger.ts` approximately every second when entries are
/// pending. Fire-and-forget from the browser side — errors here are swallowed
/// so a log-flush failure never breaks the renderer.
#[tauri::command]
fn append_shell_log(entries: Vec<RendererLogEntry>) {
    for e in entries {
        match e.level.as_str() {
            "error" => tracing::error!(target: "nexus_shell::renderer", ts = e.ts, "{}", e.message),
            "warn"  => tracing::warn! (target: "nexus_shell::renderer", ts = e.ts, "{}", e.message),
            "debug" => tracing::debug!(target: "nexus_shell::renderer", ts = e.ts, "{}", e.message),
            _       => tracing::info! (target: "nexus_shell::renderer", ts = e.ts, "{}", e.message),
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        // BL-043 — global hotkey for the quick-capture overlay. The
        // frontend `nexus.memory` plugin registers/unregisters specific
        // accelerators at activate/deactivate time; this just wires the
        // plugin into the Builder so the JS bridge can talk to it.
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Persist window size, position, and maximized state across
        // launches. The plugin saves on close/move/resize/maximize and
        // restores on window creation; without it the size hard-coded
        // in tauri.conf.json (1280x800) is reapplied every boot.
        // Persist window geometry/maximize, but NOT the `decorated` flag —
        // we want the borderless config in tauri.conf.json (`decorations:
        // false`) to be authoritative every launch. Otherwise a one-time
        // decorated-true (e.g. WM-injected on WSLg) gets baked into the
        // state file and overrides the config forever.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        - tauri_plugin_window_state::StateFlags::DECORATIONS,
                )
                .build(),
        )
        .manage(bridge::KernelRuntime::new())
        // E2E-only: if NEXUS_E2E_VAULT is set, init + boot the kernel here
        // directly (bypassing the webview IPC path). Webdriver-injected
        // `invoke()` calls fail with "Origin header is not a valid URL" on
        // Tauri v2 + tauri-driver BiDi, so we pre-seed the runtime from
        // Rust. We also write the vault into shell-state's last_forge_path
        // so the launcher's recents / frontend restore paths see it too.
        .setup(|app| {
            // Dev-only: auto-open devtools so an unresponsive/blank main
            // window (e.g. WSLg WebKit rendering hiccup) still gives us a
            // separate inspector window we can read the JS console from.
            #[cfg(debug_assertions)]
            if let Some(main) = app.get_webview_window("main") {
                main.open_devtools();
            }

            // WI-13 follow-up: bridge OS-level `nexus://…` deep-links into
            // the frontend's `uriHandlerRegistry`. The plugin delivers one
            // event per OS open — we emit each URL as a string payload on
            // `nexus:url-opened` and let the frontend construct the `URL`
            // and call `dispatch()`. Errors in emit are logged but do not
            // fail startup.
            let app_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let s = url.to_string();
                    if let Err(e) = app_handle.emit(DEEP_LINK_EVENT, s.clone()) {
                        eprintln!("[deep-link] emit({DEEP_LINK_EVENT}) failed for {s}: {e}");
                    }
                }
            });

            let vault = match std::env::var("NEXUS_E2E_VAULT") {
                Ok(v) if !v.is_empty() => v,
                _ => return Ok(()),
            };
            eprintln!("[e2e-setup] NEXUS_E2E_VAULT={vault} — seeding kernel");

            let vault_path = std::path::PathBuf::from(&vault);
            let runtime_state = app.state::<bridge::KernelRuntime>();

            // Cap the block so a hung init/boot can't freeze app startup.
            let boot_result = tauri::async_runtime::block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    async {
                        bridge::init_forge(vault.clone(), None).await?;
                        runtime_state.boot_at(&vault_path).await
                    },
                )
                .await
                .map_err(|_| "timed out waiting for init_forge/boot_kernel".to_string())
                .and_then(|r| r)
            });

            match boot_result {
                Ok(()) => {
                    eprintln!("[e2e-setup] kernel booted at {vault}");
                    // Write to shell-state so the launcher's recents and any
                    // "restore last forge" path reflects the e2e vault.
                    if let Err(e) = persistence::write_last_forge_path(
                        app.handle().clone(),
                        vault.clone(),
                    ) {
                        eprintln!("[e2e-setup] write_last_forge_path failed: {e}");
                    }
                }
                Err(e) => {
                    eprintln!("[e2e-setup] kernel boot failed: {e} (continuing)");
                }
            }

            Ok(())
        })
        // Fire the kernel shutdown when the user closes a window. Fire-and-
        // forget for now — Tauri 2's `CloseRequested` has an `api` handle we
        // could use to delay the actual close until shutdown completes, but
        // that adds complexity we don't need until something demonstrates a
        // race. A warning is logged if shutdown fails so it at least shows up
        // in the dev console.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let app_handle = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    let runtime = app_handle.state::<bridge::KernelRuntime>();
                    if let Err(e) = runtime.shutdown().await {
                        eprintln!("[shutdown] kernel shutdown failed: {e}");
                    }
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            scan_plugin_directory,
            scan_plugin_directory_at,
            set_plugin_enabled,
            get_plugin_granted_capabilities,
            set_plugin_granted_capabilities,
            path_exists,
            append_shell_log,
            persistence::get_shell_state,
            persistence::save_shell_state,
            persistence::write_last_forge_path,
            persistence::forget_forge_path,
            bridge::init_forge,
            bridge::boot_kernel,
            bridge::shutdown_kernel,
            bridge::revoke_plugin_capability,
            bridge::kernel_invoke,
            bridge::kernel_subscribe,
            bridge::kernel_unsubscribe,
            bridge::kernel_is_booted,
            windows::popout_window,
            windows::close_popout_window,
            windows::list_popout_windows,
            windows::get_popout_window_bounds,
            windows::set_popout_window_bounds,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn granted_caps_round_trip_to_kernel_format() {
        let dir = tempfile::TempDir::new().unwrap();
        let entry = GrantedCapabilityEntry {
            version: "1.2.3".into(),
            capabilities: vec!["fs.read".into(), "net.http".into()],
        };
        write_granted_entry(dir.path(), &entry).unwrap();

        // File on disk must use the kernel's `GrantedCapsFile` shape
        // (`version` + `granted` array, NOT `capabilities`).
        let raw = std::fs::read_to_string(dir.path().join(GRANTED_CAPS_FILENAME)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["version"], "1.2.3");
        assert!(parsed["granted"].is_array());
        // Round-trip back through our reader.
        let read = read_granted_entry(dir.path());
        assert_eq!(read.version, "1.2.3");
        assert_eq!(read.capabilities.len(), 2);
        assert!(read.capabilities.contains(&"fs.read".to_string()));
        assert!(read.capabilities.contains(&"net.http".to_string()));
    }

    #[test]
    fn granted_caps_missing_file_yields_empty_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let read = read_granted_entry(dir.path());
        assert_eq!(read.version, "");
        assert!(read.capabilities.is_empty());
    }

    #[test]
    fn granted_caps_corrupt_file_yields_empty_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join(GRANTED_CAPS_FILENAME), b"{ not json").unwrap();
        let read = read_granted_entry(dir.path());
        assert_eq!(read.version, "");
        assert!(read.capabilities.is_empty());
    }

    #[test]
    fn write_granted_entry_dedupes_and_sorts() {
        let dir = tempfile::TempDir::new().unwrap();
        let entry = GrantedCapabilityEntry {
            version: "0.1.0".into(),
            capabilities: vec![
                "process.spawn".into(),
                "fs.read".into(),
                "process.spawn".into(), // dupe
                "fs.read.external".into(),
            ],
        };
        write_granted_entry(dir.path(), &entry).unwrap();
        let read = read_granted_entry(dir.path());
        assert_eq!(
            read.capabilities,
            vec!["fs.read".to_string(), "fs.read.external".into(), "process.spawn".into()]
        );
    }

    #[test]
    fn set_plugin_granted_capabilities_rejects_missing_dir() {
        let err = set_plugin_granted_capabilities(
            "/no/such/path/definitely".into(),
            "1.0.0".into(),
            vec!["fs.read".into()],
        )
        .unwrap_err();
        assert!(err.contains("plugin_dir"));
    }

    #[test]
    fn community_manifest_deserialises_capabilities_field() {
        let json = r#"{
            "id": "com.example.thing",
            "name": "Thing",
            "version": "1.0.0",
            "main": "index.js",
            "apiVersion": 1,
            "capabilities": ["FsRead", "NetHttp"]
        }"#;
        let m: CommunityPluginManifest = serde_json::from_str(json).unwrap();
        let caps = m.capabilities.expect("capabilities missing");
        assert_eq!(caps, vec!["FsRead".to_string(), "NetHttp".into()]);
    }

    #[test]
    fn community_manifest_capabilities_optional() {
        let json = r#"{
            "id": "com.example.thing",
            "name": "Thing",
            "version": "1.0.0",
            "main": "index.js"
        }"#;
        let m: CommunityPluginManifest = serde_json::from_str(json).unwrap();
        assert!(m.capabilities.is_none());
    }
}
