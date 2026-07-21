//! Plugin project scaffolding: generates starter plugin directories from
//! embedded templates for both `core` and `community` trust levels.

use std::path::Path;

use regex_lite::Regex;

use crate::PluginError;

// ─── Plugin ID validation regex ───────────────────────────────────────────────

/// Pattern: `<segment>.<segment>` where each segment is `[a-z0-9]+([-._][a-z0-9]+)*`.
const PLUGIN_ID_PATTERN: &str = r"^[a-z0-9]+([-._][a-z0-9]+)*\.[a-z0-9]+([-._][a-z0-9]+)*$";

// ─── Public types ─────────────────────────────────────────────────────────────

/// Selects which template variant to use when scaffolding a plugin project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginTemplate {
    /// Core plugin template: maximum trust, zero fuel limit, no capability
    /// declarations required.
    Core,
    /// Community plugin template: sandboxed WASM trust level with a fuel cap
    /// and explicit `kv.read` / `kv.write` capability declarations.
    Community,
    /// Script plugin template: sandboxed JS/TS community plugin that runs
    /// inside a null-origin iframe and consumes `@nexus/extension-api`.
    /// Emits `plugin.json`, `index.ts`, `package.json`, `tsconfig.json`, and
    /// `README.md`. This is the modern authoring path for Phase 3c+ sandboxed
    /// community plugins.
    Script,
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

// ─── `@nexus/extension-api` pin ───────────────────────────────────────────────

/// Version of `@nexus/extension-api` that scaffolded `script` projects pin to.
///
/// Scaffolded plugins live *outside* the pnpm workspace, so we can't use
/// `workspace:*` — the version is baked into the template at scaffold time.
/// Bump this in lockstep with `packages/nexus-extension-api/package.json`;
/// the unit test `scaffold_script_package_pins_extension_api` guards against
/// stale pins by requiring it to parse as a semver caret range.
const EXTENSION_API_VERSION: &str = "^1.0.0";

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
on_load = true
on_init = true
on_start = true
on_stop = true
on_unload = true
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
on_load = true
on_init = true
on_start = true
on_stop = true
on_unload = true
"#;

// ─── Script template (sandboxed JS/TS community plugin) ──────────────────────

const SCRIPT_PLUGIN_JSON: &str = include_str!("../templates/script/plugin.json");
const SCRIPT_INDEX_TS: &str = include_str!("../templates/script/index.ts");
const SCRIPT_README_MD: &str = include_str!("../templates/script/README.md");
const SCRIPT_PACKAGE_JSON: &str = include_str!("../templates/script/package.json");
const SCRIPT_TSCONFIG_JSON: &str = include_str!("../templates/script/tsconfig.json");
/// C89 (#442) — smoke test exercising `activate(ctx)` against a fake
/// `SandboxedPluginContext`, so scaffolded plugins have a real test from
/// the start instead of zero test coverage.
const SCRIPT_INDEX_TEST_TS: &str = include_str!("../templates/script/index.test.ts");
/// C89 (#442) — separate tsconfig for test files (adds `types: ["node"]`
/// for `node:test`/`node:assert`, mirroring `shell/tsconfig.test.json`).
const SCRIPT_TSCONFIG_TEST_JSON: &str = include_str!("../templates/script/tsconfig.test.json");
/// C89 (#442) — GitHub Actions workflow running typecheck/test/build,
/// written to `.github/workflows/ci.yml` in the scaffolded output.
const SCRIPT_CI_YML: &str = include_str!("../templates/script/ci.yml");
/// C89 (#442) — happy-dom global shim, registered via `node --import`
/// before the test file loads. `index.ts` calls `bootstrapSandboxedPlugin`
/// at module scope, which expects real `window`/`postMessage` globals.
const SCRIPT_TEST_SETUP_TS: &str = include_str!("../templates/script/test-setup.ts");

const SRC_LIB_RS_TEMPLATE: &str = r#"//! {{plugin-name}} — Nexus plugin.

use std::alloc::{alloc, Layout};

// ─── Allocator export ─────────────────────────────────────────────────────────

/// WASM allocator required by the Nexus host to copy data into WASM memory.
#[no_mangle]
pub extern "C" fn nexus_alloc(size: u32) -> u32 {
    if size == 0 {
        return 0;
    }
    unsafe {
        let layout = Layout::from_size_align(size as usize, 1).unwrap();
        alloc(layout) as u32
    }
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// Primary dispatch entry-point called by the Nexus host.
///
/// `handler_id` selects which handler to invoke:
/// - `0` → `on_init`
/// - `1` → `on_start`
/// - `2` → `on_stop`
/// - `3` → `on_load`
/// - `4` → `on_enable`
/// - `5` → `on_disable`
/// - `6` → `on_unload`
/// - `7` → `on_settings_changed`
/// - `100`+ → user-defined handlers
///
/// `ptr` / `len` point to a UTF-8 JSON payload in WASM linear memory.
///
/// Returns a packed `u64`: high 32 bits = result pointer, low 32 bits = result
/// byte length. The result bytes are valid JSON.
#[no_mangle]
pub extern "C" fn nexus_dispatch(handler_id: u32, ptr: u32, len: u32) -> u64 {
    let input = if len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) }
    };

    let result: Vec<u8> = match handler_id {
        0 => on_init(input),
        1 => on_start(input),
        2 => on_stop(input),
        3 => on_load(input),
        4 => on_enable(input),
        5 => on_disable(input),
        6 => on_unload(input),
        7 => on_settings_changed(input),
        100 => echo(input),
        _ => b"{\"error\":\"unknown handler\"}".to_vec(),
    };

    write_result(&result)
}

// ─── Lifecycle handlers ───────────────────────────────────────────────────────

fn on_init(_input: &[u8]) -> Vec<u8> { b"{}".to_vec() }
fn on_start(_input: &[u8]) -> Vec<u8> { b"{}".to_vec() }
fn on_stop(_input: &[u8]) -> Vec<u8> { b"{}".to_vec() }
fn on_load(_input: &[u8]) -> Vec<u8> { b"{}".to_vec() }
fn on_enable(_input: &[u8]) -> Vec<u8> { b"{}".to_vec() }
fn on_disable(_input: &[u8]) -> Vec<u8> { b"{}".to_vec() }
fn on_unload(_input: &[u8]) -> Vec<u8> { b"{}".to_vec() }
fn on_settings_changed(input: &[u8]) -> Vec<u8> { input.to_vec() }

// ─── Example handler: echo ────────────────────────────────────────────────────

/// Returns the input JSON payload unchanged.
fn echo(input: &[u8]) -> Vec<u8> {
    input.to_vec()
}

// ─── Helper ───────────────────────────────────────────────────────────────────

/// Allocate `bytes` in WASM linear memory and return a packed `u64` result.
///
/// High 32 bits = pointer to the result, low 32 bits = byte length.
fn write_result(bytes: &[u8]) -> u64 {
    let result_ptr = nexus_alloc(bytes.len() as u32);
    if result_ptr != 0 && !bytes.is_empty() {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), result_ptr as *mut u8, bytes.len());
        }
    }
    ((result_ptr as u64) << 32) | (bytes.len() as u64)
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

    // 3. Dispatch per-template. `Script` produces a JS/TS project; the two
    //    WASM variants (Core / Community) share a Cargo + manifest + lib.rs
    //    layout.
    match template {
        PluginTemplate::Script => scaffold_script(output_dir, config),
        PluginTemplate::Core | PluginTemplate::Community => {
            scaffold_wasm(output_dir, template, config)
        }
    }
}

/// Emit the WASM-flavored project (Core / Community): Cargo.toml, manifest.toml,
/// src/lib.rs.
fn scaffold_wasm(
    output_dir: &Path,
    template: PluginTemplate,
    config: &ScaffoldConfig,
) -> Result<(), PluginError> {
    let src_dir = output_dir.join("src");
    std::fs::create_dir_all(&src_dir)?;

    write_file(
        &output_dir.join("Cargo.toml"),
        apply_substitutions(CARGO_TOML_TEMPLATE, config),
    )?;

    let manifest_template = match template {
        PluginTemplate::Core => MANIFEST_TOML_CORE,
        PluginTemplate::Community => MANIFEST_TOML_COMMUNITY,
        PluginTemplate::Script => unreachable!("script handled by scaffold_script"),
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

/// Emit the script (sandboxed JS/TS) project: plugin.json, index.ts, README.md,
/// package.json, tsconfig.json. No `src/` subdirectory — authors edit
/// `index.ts` at the project root.
fn scaffold_script(output_dir: &Path, config: &ScaffoldConfig) -> Result<(), PluginError> {
    std::fs::create_dir_all(output_dir)?;

    write_file(
        &output_dir.join("plugin.json"),
        apply_substitutions(SCRIPT_PLUGIN_JSON, config),
    )?;
    write_file(
        &output_dir.join("index.ts"),
        apply_substitutions(SCRIPT_INDEX_TS, config),
    )?;
    write_file(
        &output_dir.join("README.md"),
        apply_substitutions(SCRIPT_README_MD, config),
    )?;
    write_file(
        &output_dir.join("package.json"),
        apply_substitutions(SCRIPT_PACKAGE_JSON, config),
    )?;
    write_file(
        &output_dir.join("tsconfig.json"),
        apply_substitutions(SCRIPT_TSCONFIG_JSON, config),
    )?;
    // C89 (#442) — test + CI scaffolding, so a fresh plugin has real
    // coverage and a gate from the start instead of neither.
    write_file(
        &output_dir.join("index.test.ts"),
        apply_substitutions(SCRIPT_INDEX_TEST_TS, config),
    )?;
    write_file(
        &output_dir.join("tsconfig.test.json"),
        apply_substitutions(SCRIPT_TSCONFIG_TEST_JSON, config),
    )?;
    write_file(
        &output_dir.join("test-setup.ts"),
        apply_substitutions(SCRIPT_TEST_SETUP_TS, config),
    )?;
    let workflows_dir = output_dir.join(".github").join("workflows");
    std::fs::create_dir_all(&workflows_dir)?;
    write_file(
        &workflows_dir.join("ci.yml"),
        apply_substitutions(SCRIPT_CI_YML, config),
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
        .replace("{{extension-api-version}}", EXTENSION_API_VERSION)
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
        assert!(
            out.join("src").join("lib.rs").exists(),
            "src/lib.rs missing"
        );
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
    fn scaffold_script_creates_expected_file_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("script-plugin");
        scaffold(&out, PluginTemplate::Script, &test_config()).unwrap();

        for f in [
            "plugin.json",
            "index.ts",
            "README.md",
            "package.json",
            "tsconfig.json",
            "index.test.ts",
            "tsconfig.test.json",
            "test-setup.ts",
            ".github/workflows/ci.yml",
        ] {
            assert!(out.join(f).exists(), "script template missing file: {f}");
        }
        // Script projects do not have a `src/` subdirectory — authors edit
        // `index.ts` at the project root.
        assert!(
            !out.join("src").exists(),
            "script template should not emit src/ directory"
        );
        // And none of the WASM-template outputs.
        assert!(
            !out.join("Cargo.toml").exists(),
            "Cargo.toml leaked into script scaffold"
        );
        assert!(
            !out.join("manifest.toml").exists(),
            "manifest.toml leaked into script scaffold"
        );
    }

    #[test]
    fn scaffold_script_manifest_is_sandboxed_json() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("script-manifest");
        scaffold(&out, PluginTemplate::Script, &test_config()).unwrap();

        let manifest = std::fs::read_to_string(out.join("plugin.json")).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&manifest).expect("plugin.json must parse as JSON");
        assert_eq!(parsed["id"], "com.example.my-plugin");
        assert_eq!(parsed["name"], "My Plugin");
        assert_eq!(parsed["apiVersion"], 1);
        assert_eq!(parsed["sandboxed"], true);
        assert_eq!(parsed["main"], "index.js");
        assert!(parsed["capabilities"].is_array());
    }

    #[test]
    fn scaffold_script_index_ts_wires_bootstrap_and_registers_one_command_one_panel() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("script-src");
        scaffold(&out, PluginTemplate::Script, &test_config()).unwrap();

        let src = std::fs::read_to_string(out.join("index.ts")).unwrap();
        assert!(
            src.contains("from '@nexus/extension-api'"),
            "index.ts must import from @nexus/extension-api"
        );
        assert!(
            src.contains("bootstrapSandboxedPlugin(plugin)"),
            "index.ts must call bootstrapSandboxedPlugin"
        );
        assert_eq!(
            src.matches("ctx.commands.register").count(),
            1,
            "script template should register exactly one command"
        );
        assert_eq!(
            src.matches("ctx.views.registerPanel").count(),
            1,
            "script template should register exactly one panel"
        );
        assert!(
            !src.contains("{{plugin-id}}") && !src.contains("{{plugin-name}}"),
            "unreplaced placeholder in index.ts"
        );
    }

    #[test]
    fn scaffold_script_package_pins_extension_api() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("script-pkg");
        scaffold(&out, PluginTemplate::Script, &test_config()).unwrap();

        let pkg_str = std::fs::read_to_string(out.join("package.json")).unwrap();
        let pkg: serde_json::Value =
            serde_json::from_str(&pkg_str).expect("package.json must parse as JSON");

        assert_eq!(pkg["name"], "com.example.my-plugin");
        let api_pin = pkg["devDependencies"]["@nexus/extension-api"]
            .as_str()
            .expect("extension-api pin must be a string");
        // Accept any caret/tilde/exact semver — but reject `workspace:*`
        // (scaffolded projects live outside the workspace).
        assert!(
            !api_pin.contains("workspace:"),
            "extension-api must not be pinned to workspace:* in scaffold; got {api_pin}"
        );
        assert!(
            api_pin.starts_with('^')
                || api_pin.starts_with('~')
                || api_pin.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "extension-api pin must be a concrete semver, got {api_pin}"
        );
        // esbuild + typescript must be present for `pnpm build`.
        assert!(pkg["devDependencies"]["esbuild"].is_string());
        assert!(pkg["devDependencies"]["typescript"].is_string());
        assert!(pkg["scripts"]["build"]
            .as_str()
            .unwrap()
            .contains("esbuild"));
    }

    #[test]
    fn scaffold_script_tsconfig_parses_and_targets_es2020() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("script-tsconfig");
        scaffold(&out, PluginTemplate::Script, &test_config()).unwrap();

        let raw = std::fs::read_to_string(out.join("tsconfig.json")).unwrap();
        let cfg: serde_json::Value =
            serde_json::from_str(&raw).expect("tsconfig.json must parse as JSON");
        assert_eq!(cfg["compilerOptions"]["target"], "ES2020");
        assert_eq!(cfg["compilerOptions"]["module"], "esnext");
        assert_eq!(cfg["compilerOptions"]["strict"], true);
    }

    #[test]
    fn scaffold_script_test_file_exercises_activate_and_has_no_unreplaced_placeholders() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("script-test");
        scaffold(&out, PluginTemplate::Script, &test_config()).unwrap();

        let test_src = std::fs::read_to_string(out.join("index.test.ts")).unwrap();
        assert!(
            // Imports the *bundled* output, not `./index.ts` directly — see
            // the file's own doc comment for why (bootstrapSandboxedPlugin's
            // DOM globals + @nexus/extension-api's bundler-only resolution).
            test_src.contains("from './.test-bundle.mjs'"),
            "index.test.ts must import the pretest-bundled plugin output"
        );
        assert!(
            test_src.contains("plugin.activate"),
            "index.test.ts must exercise activate()"
        );
        assert!(
            !test_src.contains("{{plugin-id}}") && !test_src.contains("{{plugin-name}}"),
            "unreplaced placeholder in index.test.ts"
        );
    }

    #[test]
    fn scaffold_script_test_setup_registers_happy_dom() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("script-test-setup");
        scaffold(&out, PluginTemplate::Script, &test_config()).unwrap();

        let setup_src = std::fs::read_to_string(out.join("test-setup.ts")).unwrap();
        assert!(
            setup_src.contains("@happy-dom/global-registrator"),
            "test-setup.ts must register the happy-dom DOM shim"
        );
    }

    #[test]
    fn scaffold_script_package_wires_test_scripts_and_devdeps() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("script-test-pkg");
        scaffold(&out, PluginTemplate::Script, &test_config()).unwrap();

        let pkg_str = std::fs::read_to_string(out.join("package.json")).unwrap();
        let pkg: serde_json::Value =
            serde_json::from_str(&pkg_str).expect("package.json must parse as JSON");

        let test_script = pkg["scripts"]["test"]
            .as_str()
            .expect("test script must exist");
        assert!(test_script.contains("node --import tsx"));
        assert!(
            test_script.contains("--import ./test-setup.ts"),
            "test script must register the happy-dom setup file"
        );
        // The bundle `index.test.ts` imports is produced by `pretest`, not
        // `build` — a separate output so tsx's .js->.ts sibling lookup
        // doesn't redirect back to unbundled source (see index.test.ts).
        assert!(pkg["scripts"]["pretest"]
            .as_str()
            .expect("pretest script must exist")
            .contains(".test-bundle.mjs"));
        assert!(pkg["scripts"]["typecheck:test"].is_string());
        assert!(pkg["devDependencies"]["tsx"].is_string());
        assert!(pkg["devDependencies"]["@types/node"].is_string());
        assert!(pkg["devDependencies"]["happy-dom"].is_string());
        assert!(pkg["devDependencies"]["@happy-dom/global-registrator"].is_string());
    }

    #[test]
    fn scaffold_script_tsconfig_test_extends_base_and_includes_test_file() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("script-tsconfig-test");
        scaffold(&out, PluginTemplate::Script, &test_config()).unwrap();

        let raw = std::fs::read_to_string(out.join("tsconfig.test.json")).unwrap();
        let cfg: serde_json::Value =
            serde_json::from_str(&raw).expect("tsconfig.test.json must parse as JSON");
        assert_eq!(cfg["extends"], "./tsconfig.json");
        assert_eq!(cfg["compilerOptions"]["types"][0], "node");
        let includes = cfg["include"]
            .as_array()
            .expect("include must be an array");
        assert!(includes.iter().any(|v| v == "*.test.ts"));
    }

    #[test]
    fn scaffold_script_ci_workflow_runs_typecheck_test_and_build() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("script-ci");
        scaffold(&out, PluginTemplate::Script, &test_config()).unwrap();

        let workflow = std::fs::read_to_string(out.join(".github/workflows/ci.yml")).unwrap();
        assert!(workflow.contains("jobs:"), "ci.yml must define jobs");
        for needle in ["pnpm install", "pnpm typecheck", "pnpm test", "pnpm build"] {
            assert!(
                workflow.contains(needle),
                "ci.yml missing expected step containing '{needle}'"
            );
        }
    }

    #[test]
    fn scaffold_script_does_not_break_wasm_templates() {
        // Regression guard: the Script branch must not regress Core/Community.
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("legacy-wasm");
        scaffold(&out, PluginTemplate::Community, &test_config()).unwrap();
        assert!(out.join("Cargo.toml").exists());
        assert!(out.join("manifest.toml").exists());
        assert!(out.join("src").join("lib.rs").exists());
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
