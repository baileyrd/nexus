//! Plugin project scaffolding: generates starter plugin directories from
//! embedded templates for both `core` and `community` trust levels.

use std::path::Path;

use regex_lite::Regex;

use crate::PluginError;

// ─── Plugin ID validation regex ───────────────────────────────────────────────

/// Pattern: `<segment>.<segment>` where each segment is `[a-z0-9]+([-._][a-z0-9]+)*`.
const PLUGIN_ID_PATTERN: &str =
    r"^[a-z0-9]+([-._][a-z0-9]+)*\.[a-z0-9]+([-._][a-z0-9]+)*$";

// ─── Public types ─────────────────────────────────────────────────────────────

/// Selects which template variant to use when scaffolding a plugin project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginTemplate {
    /// Core plugin template: maximum trust, zero fuel limit, no capability
    /// declarations required.
    Core,
    /// Community plugin template: sandboxed trust level with a fuel cap and
    /// explicit `kv.read` / `kv.write` capability declarations.
    Community,
}

/// Input configuration consumed by [`scaffold`].
#[derive(Debug, Clone)]
pub struct ScaffoldConfig {
    /// Reverse-DNS plugin identifier, e.g. `com.example.my-plugin`.
    pub plugin_id: String,
    /// Human-readable display name, e.g. `"My Plugin"`.
    pub plugin_name: String,
    /// Author name or e-mail address.
    pub author: String,
    /// Short description of what the plugin does.
    pub description: String,
}

// ─── Templates ────────────────────────────────────────────────────────────────

const CARGO_TOML_TEMPLATE: &str = r#"[package]
name = "{{plugin-id}}"
version = "0.1.0"
edition = "2021"
authors = ["{{author}}"]
description = "{{description}}"

[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "s"
lto = true
"#;

const MANIFEST_TOML_CORE: &str = r#"[plugin]
id = "{{plugin-id}}"
name = "{{plugin-name}}"
version = "0.1.0"
description = "{{description}}"
trust_level = "core"
api_version = "1"

[wasm]
module = "plugin.wasm"
memory_mb = 16
fuel = 0

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#;

const MANIFEST_TOML_COMMUNITY: &str = r#"[plugin]
id = "{{plugin-id}}"
name = "{{plugin-name}}"
version = "0.1.0"
description = "{{description}}"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read", "kv.write"]

[wasm]
module = "plugin.wasm"
memory_mb = 16
fuel = 10000000

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#;

const SRC_LIB_RS_TEMPLATE: &str = r#"//! {{plugin-name}} — Nexus plugin.

use std::alloc::{GlobalAlloc, Layout, System};

// ─── Allocator export ─────────────────────────────────────────────────────────

/// WASM allocator shim required by the Nexus host.
///
/// # Safety
/// Delegates directly to the system allocator.
#[no_mangle]
pub unsafe extern "C" fn nexus_alloc(size: u32, align: u32) -> u32 {
    let layout = match Layout::from_size_align(size as usize, align as usize) {
        Ok(l) => l,
        Err(_) => return 0,
    };
    System.alloc(layout) as u32
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// Primary dispatch entry-point called by the Nexus host.
///
/// `handler_id` selects which handler to invoke:
/// - `0`   → `on_init`
/// - `1`   → `on_start`
/// - `2`   → `on_stop`
/// - `100` → echo (example handler)
///
/// `ptr` / `len` point to a UTF-8 JSON payload in WASM linear memory.
/// Returns a pointer to a null-terminated JSON result string, or 0 on error.
#[no_mangle]
pub extern "C" fn nexus_dispatch(handler_id: u32, ptr: u32, len: u32) -> u32 {
    let input = if len == 0 {
        b"null".as_slice()
    } else {
        unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) }
    };

    let result: &[u8] = match handler_id {
        0 => on_init(input),
        1 => on_start(input),
        2 => on_stop(input),
        100 => echo(input),
        _ => b"null",
    };

    write_result(result)
}

// ─── Lifecycle handlers ───────────────────────────────────────────────────────

fn on_init(_input: &[u8]) -> &'static [u8] {
    b"null"
}

fn on_start(_input: &[u8]) -> &'static [u8] {
    b"null"
}

fn on_stop(_input: &[u8]) -> &'static [u8] {
    b"null"
}

// ─── Example handler: echo ────────────────────────────────────────────────────

/// Returns the input JSON payload unchanged.
fn echo(input: &[u8]) -> &[u8] {
    input
}

// ─── Helper ───────────────────────────────────────────────────────────────────

/// Leak `bytes` into WASM linear memory and return its pointer.
///
/// The host is responsible for reading the result before the next dispatch
/// call overwrites the static buffer.
fn write_result(bytes: &[u8]) -> u32 {
    // Use a static mutable buffer so we don't need a heap allocator for
    // small results.  For production use you would Box::leak or similar.
    static mut BUF: [u8; 65536] = [0u8; 65536];
    let len = bytes.len().min(65535);
    unsafe {
        BUF[..len].copy_from_slice(&bytes[..len]);
        BUF[len] = 0;
        BUF.as_ptr() as u32
    }
}
"#;

// ─── Public API ───────────────────────────────────────────────────────────────

/// Return a reference to the compiled plugin-ID regex, compiling it once.
fn plugin_id_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(PLUGIN_ID_PATTERN).expect("hard-coded regex is valid"))
}

/// Generate a plugin project at `output_dir` from the chosen `template`.
///
/// Creates:
/// - `output_dir/Cargo.toml`
/// - `output_dir/manifest.toml`
/// - `output_dir/src/lib.rs`
///
/// All `{{placeholder}}` tokens are substituted with values from `config`.
///
/// # Errors
///
/// - [`PluginError::ManifestValidation`] — `config.plugin_id` does not match
///   the required reverse-DNS pattern.
/// - [`PluginError::Io`] — `output_dir` already exists and is non-empty, or
///   any file-system operation fails.
pub fn scaffold(
    output_dir: &Path,
    template: PluginTemplate,
    config: &ScaffoldConfig,
) -> Result<(), PluginError> {
    // 1. Validate plugin_id.
    let re = plugin_id_regex();
    if !re.is_match(&config.plugin_id) {
        return Err(PluginError::ManifestValidation {
            plugin_id: config.plugin_id.clone(),
            reason: format!(
                "plugin_id '{}' does not match required pattern {}",
                config.plugin_id, PLUGIN_ID_PATTERN
            ),
        });
    }

    // 2. Ensure output_dir is absent or empty.
    if output_dir.exists() {
        let is_empty = output_dir
            .read_dir()
            .map_err(PluginError::Io)?
            .next()
            .is_none();
        if !is_empty {
            return Err(PluginError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "output directory '{}' already exists and is non-empty",
                    output_dir.display()
                ),
            )));
        }
    }

    // 3. Create directory layout.
    let src_dir = output_dir.join("src");
    std::fs::create_dir_all(&src_dir)?;

    // 4. Render and write files.
    write_file(
        &output_dir.join("Cargo.toml"),
        apply_substitutions(CARGO_TOML_TEMPLATE, config),
    )?;

    let manifest_template = match template {
        PluginTemplate::Core => MANIFEST_TOML_CORE,
        PluginTemplate::Community => MANIFEST_TOML_COMMUNITY,
    };
    write_file(
        &output_dir.join("manifest.toml"),
        apply_substitutions(manifest_template, config),
    )?;

    write_file(
        &src_dir.join("lib.rs"),
        apply_substitutions(SRC_LIB_RS_TEMPLATE, config),
    )?;

    Ok(())
}

// ─── Private helpers ──────────────────────────────────────────────────────────

fn apply_substitutions(template: &str, config: &ScaffoldConfig) -> String {
    template
        .replace("{{plugin-id}}", &config.plugin_id)
        .replace("{{plugin-name}}", &config.plugin_name)
        .replace("{{author}}", &config.author)
        .replace("{{description}}", &config.description)
}

fn write_file(path: &Path, contents: String) -> Result<(), PluginError> {
    std::fs::write(path, contents)?;
    Ok(())
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ScaffoldConfig {
        ScaffoldConfig {
            plugin_id: "com.example.my-plugin".to_string(),
            plugin_name: "My Plugin".to_string(),
            author: "Test Author".to_string(),
            description: "A test plugin.".to_string(),
        }
    }

    #[test]
    fn scaffold_creates_directory_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("my-plugin");
        scaffold(&out, PluginTemplate::Community, &test_config()).unwrap();

        assert!(out.join("Cargo.toml").exists(), "Cargo.toml missing");
        assert!(out.join("manifest.toml").exists(), "manifest.toml missing");
        assert!(out.join("src").join("lib.rs").exists(), "src/lib.rs missing");
    }

    #[test]
    fn scaffold_community_manifest_has_community_trust_level() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("comm-plugin");
        scaffold(&out, PluginTemplate::Community, &test_config()).unwrap();

        let manifest = std::fs::read_to_string(out.join("manifest.toml")).unwrap();
        assert!(
            manifest.contains(r#"trust_level = "community""#),
            "expected community trust_level, got:\n{manifest}"
        );
        assert!(
            manifest.contains("fuel = 10000000"),
            "expected fuel = 10000000, got:\n{manifest}"
        );
    }

    #[test]
    fn scaffold_core_manifest_has_core_trust_level() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("core-plugin");
        scaffold(&out, PluginTemplate::Core, &test_config()).unwrap();

        let manifest = std::fs::read_to_string(out.join("manifest.toml")).unwrap();
        assert!(
            manifest.contains(r#"trust_level = "core""#),
            "expected core trust_level, got:\n{manifest}"
        );
        assert!(
            manifest.contains("fuel = 0"),
            "expected fuel = 0, got:\n{manifest}"
        );
    }

    #[test]
    fn scaffold_substitutes_plugin_id() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("sub-id-plugin");
        scaffold(&out, PluginTemplate::Community, &test_config()).unwrap();

        let manifest = std::fs::read_to_string(out.join("manifest.toml")).unwrap();
        assert!(
            manifest.contains("com.example.my-plugin"),
            "plugin_id not found in manifest"
        );
        assert!(
            !manifest.contains("{{plugin-id}}"),
            "unreplaced placeholder found in manifest"
        );

        let cargo = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(
            cargo.contains("com.example.my-plugin"),
            "plugin_id not found in Cargo.toml"
        );
        assert!(
            !cargo.contains("{{plugin-id}}"),
            "unreplaced placeholder found in Cargo.toml"
        );
    }

    #[test]
    fn scaffold_substitutes_author() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("author-plugin");
        scaffold(&out, PluginTemplate::Community, &test_config()).unwrap();

        let cargo = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(
            cargo.contains("Test Author"),
            "author not found in Cargo.toml"
        );
        assert!(
            !cargo.contains("{{author}}"),
            "unreplaced author placeholder found in Cargo.toml"
        );
    }

    #[test]
    fn scaffold_rejects_invalid_plugin_id() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("bad-plugin");

        let config = ScaffoldConfig {
            plugin_id: "INVALID_ID".to_string(),
            plugin_name: "Bad Plugin".to_string(),
            author: "Nobody".to_string(),
            description: "Should fail.".to_string(),
        };

        let err = scaffold(&out, PluginTemplate::Community, &config).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { .. }),
            "expected ManifestValidation, got: {err:?}"
        );
        assert!(
            !out.exists(),
            "output directory should not have been created"
        );
    }
}
