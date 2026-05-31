//! BL-055 — `run_saved` handler. Looks up a saved command by slug
//! and spawns a fresh PTY session running its `shell_cmd` under its
//! `shell`. Split out of `core_plugin.rs` by SD-03 terminal chunk 5.

use std::path::PathBuf;

use crate::core_plugin::{CreateSessionResponse, RunSavedArgs, TerminalCorePlugin};
use crate::memory::MemoryLimits;
use crate::server::{ServerSpawnConfig, TerminalServer};
use crate::shell::ShellSpec;
use nexus_plugins::PluginError;

use super::shared::{crate_err, exec_err, parse_args, poisoned, to_value};

impl TerminalCorePlugin {
    pub(crate) fn dispatch_run_saved(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: RunSavedArgs = parse_args(args, "run_saved")?;

        let saved = {
            let store = self.saved_store()?.lock().map_err(poisoned)?;
            store.get(&a.slug).map_err(crate_err)?.ok_or_else(|| {
                exec_err(format!(
                    "run_saved: no saved command with slug '{}'",
                    a.slug
                ))
            })?
        };

        // Resolve the working dir — caller override beats the saved
        // value, both can fall back to inheriting the parent cwd.
        let working_dir = a
            .working_dir
            .or_else(|| saved.working_dir.clone())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);

        // Run the saved cmd through `<shell> -c "<cmd>"`. POSIX shells,
        // pwsh (`-Command`), and cmd.exe (`/C`) all need a different
        // flag for non-interactive invocation; pick the right one
        // based on the shell's basename. Unknown shells fall back to
        // `-c` which is the POSIX convention.
        //
        // BL-056 — `command` overrides `shell_cmd` so workflow's
        // `run_adhoc` step can reuse the saved profile (shell, cwd,
        // env) with a fresh command line per run.
        let cmd_line = a.command.unwrap_or_else(|| saved.shell_cmd.clone());
        let shell_args = vec![one_shot_flag(&saved.shell).to_string(), cmd_line];

        // env_vars is HashMap<String, String> on SavedCommand; the
        // server expects Vec<(String, String)>. Order is irrelevant
        // (cmd-line env layering is left-to-right but the spawn
        // already merges over the inherited env).
        let env: Vec<(String, String)> = saved
            .env_vars
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let cfg = ServerSpawnConfig {
            name: Some(format!("saved:{}", saved.slug)),
            shell: Some(ShellSpec {
                program: PathBuf::from(saved.shell.clone()),
                args: shell_args,
            }),
            working_dir,
            env,
        };

        let id = self
            .server
            .lock()
            .map_err(poisoned)?
            .create_session(cfg)
            .map_err(crate_err)?;

        // BL-061 follow-up — pin the saved command's memory_limit_mb
        // onto this freshly-spawned session before the poller's next
        // round. The single-knob saved-command field maps to the
        // hard kill threshold (no separate soft warn — when a user
        // sets a per-command limit they're being explicit; a soft
        // warning at half doesn't add value). Applies only when both
        // the limit is set AND a memory monitor is wired; without the
        // monitor there's nothing to track against and the override
        // is silently a no-op (matches pre-BL-061 behaviour for
        // plugins built without `with_memory_monitor`).
        if let (Some(limit_mb), Some(memory)) = (saved.memory_limit_mb, self.memory.as_ref()) {
            let new_limits = MemoryLimits {
                soft_mb: None,
                hard_mb: Some(limit_mb),
            };
            let mut mem = memory.lock().map_err(poisoned)?;
            mem.pending_overrides
                .insert(id.as_str().to_string(), new_limits);
            // Race-proof: the poller may have already tracked this
            // session under the bootstrap-wide default (the override
            // was staged after `create_session` returned). If so,
            // update the live monitor entry in place so the wrong
            // defaults don't kill the session before our override
            // applies on the next round.
            if let Some(&pid) = mem.session_pid.get(id.as_str()) {
                mem.monitor.set_limits(pid, new_limits);
            }
        }

        to_value(
            &CreateSessionResponse {
                id: id.as_str().to_string(),
            },
            "run_saved",
        )
    }
}

/// Pick the right "run a single command and exit" flag for `shell`.
/// POSIX shells use `-c`; `pwsh`/`powershell.exe` use `-Command`;
/// `cmd.exe` uses `/C`. Anything unrecognised falls back to `-c`.
pub(crate) fn one_shot_flag(shell: &str) -> &'static str {
    let basename = std::path::Path::new(shell)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.strip_suffix(".exe").unwrap_or(s))
        .unwrap_or(shell);
    match basename.to_ascii_lowercase().as_str() {
        "cmd" => "/C",
        "pwsh" | "powershell" => "-Command",
        _ => "-c",
    }
}
