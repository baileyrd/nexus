//! Session-lifecycle handlers — `create_session`, `close_session`,
//! `resize`, `open_in_terminal`. Split out of `core_plugin.rs` by
//! SD-03 terminal chunk 2.

use std::path::PathBuf;

use crate::core_plugin::TerminalCorePlugin;
use crate::ipc::{
    CreateSessionArgs, CreateSessionResponse, RenameSessionArgs, ResizeArgs, SessionIdArgs,
};
use crate::server::{ServerSpawnConfig, TerminalServer};
use crate::session::SessionId;
use crate::shell::ShellSpec;
use nexus_plugins::PluginError;

use super::shared::{crate_err, exec_err, parse_args, poisoned, to_value};

impl TerminalCorePlugin {
    pub(crate) fn dispatch_create_session(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: CreateSessionArgs = parse_args(args, "create_session")?;
        let shell = a.shell.map(|p| ShellSpec {
            program: PathBuf::from(p),
            args: a.shell_args,
        });
        let cfg = ServerSpawnConfig {
            name: a.name,
            shell,
            working_dir: a.working_dir.map(PathBuf::from),
            env: a.env,
            // IPC-spawned sessions are not sandboxed (sandbox + bundled shell
            // are opt-in by programmatic callers; see ServerSpawnConfig).
            ..Default::default()
        };
        let id = self
            .server
            .lock()
            .map_err(poisoned)?
            .create_session(cfg)
            .map_err(crate_err)?;
        to_value(
            &CreateSessionResponse {
                id: id.as_str().to_string(),
            },
            "create_session",
        )
    }

    pub(crate) fn dispatch_close_session(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SessionIdArgs = parse_args(args, "close_session")?;
        let id = SessionId::from_string(a.id);
        self.server
            .lock()
            .map_err(poisoned)?
            .close_session(&id)
            .map_err(crate_err)?;
        // Drop the per-session emitter state so the map doesn't grow
        // unboundedly across long-running plugin instances. The drainer's
        // next round won't see this id from `list_sessions`, so the
        // entry is unreachable anyway.
        if let Ok(mut em) = self.emitters.lock() {
            em.remove(&id);
        }
        Ok(serde_json::Value::Null)
    }

    pub(crate) fn dispatch_resize(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ResizeArgs = parse_args(args, "resize")?;
        let id = SessionId::from_string(a.id);
        // Clamp zero dimensions — most tty ioctls reject them and the
        // resulting error would be opaque to the caller. xterm's fit
        // addon can occasionally propose zero before layout settles.
        let cols = a.cols.max(1);
        let rows = a.rows.max(1);
        self.server
            .lock()
            .map_err(poisoned)?
            .resize(&id, cols, rows)
            .map_err(crate_err)?;
        Ok(serde_json::Value::Null)
    }

    pub(crate) fn dispatch_rename_session(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: RenameSessionArgs = parse_args(args, "rename_session")?;
        let id = SessionId::from_string(a.id);
        self.server
            .lock()
            .map_err(poisoned)?
            .rename_session(&id, &a.name)
            .map_err(crate_err)?;
        Ok(serde_json::Value::Null)
    }

    /// BL-059 — open the saved command's `working_dir` in the user's
    /// preferred external terminal emulator. Optional `priority` arg
    /// overrides [`crate::DEFAULT_PRIORITY`] with a `snake_case` list.
    pub(crate) fn dispatch_open_in_terminal(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        use crate::external_terminal::{
            launch_spec, parse_kind, pick_first_available, spawn_detached, which_in_path,
            DEFAULT_PRIORITY,
        };

        #[derive(serde::Deserialize)]
        struct OpenInTerminalArgs {
            slug: String,
            #[serde(default)]
            priority: Option<Vec<String>>,
        }
        let a: OpenInTerminalArgs = parse_args(args, "open_in_terminal")?;

        let saved = {
            let store = self.saved_store()?.lock().map_err(poisoned)?;
            store.get(&a.slug).map_err(crate_err)?.ok_or_else(|| {
                exec_err(format!(
                    "open_in_terminal: no saved command with slug '{}'",
                    a.slug
                ))
            })?
        };

        let working_dir_str = saved
            .working_dir
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                exec_err(format!(
                    "open_in_terminal: saved command '{}' has no working_dir",
                    saved.slug
                ))
            })?;
        let working_dir = std::path::PathBuf::from(&working_dir_str);

        // Translate the (optional) caller-supplied priority into typed
        // kinds, falling back to the built-in default. Unknown tags are
        // silently dropped — the priority list shouldn't be a place
        // where a typo blocks the whole launch.
        let priority: Vec<crate::external_terminal::TerminalKind> = match a.priority {
            Some(names) => names.iter().filter_map(|n| parse_kind(n)).collect(),
            None => DEFAULT_PRIORITY.to_vec(),
        };

        let (kind, spec) =
            pick_first_available(&priority, launch_spec, which_in_path, &working_dir).ok_or_else(
                || {
                    exec_err(
                        "open_in_terminal: no supported terminal emulator found on PATH \
                     (tried the configured priority list)"
                            .to_string(),
                    )
                },
            )?;

        spawn_detached(&spec).map_err(|e| {
            exec_err(format!(
                "open_in_terminal: spawning {program} failed: {e}",
                program = spec.program,
            ))
        })?;

        Ok(serde_json::json!({
            "kind": kind,
            "program": spec.program,
            "args": spec.args,
            "working_dir": working_dir_str,
        }))
    }
}
