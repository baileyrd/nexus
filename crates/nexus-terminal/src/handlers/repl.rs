//! BL-142 REPL handlers — `repl_start`, `repl_eval`, `repl_stop`,
//! `repl_list`. Split out of `core_plugin.rs` by SD-03 terminal
//! chunk 4.

use std::path::PathBuf;

use crate::core_plugin::{
    ReplEvalArgs, ReplInfo, ReplStartArgs, ReplStartResponse, SessionIdArgs, TerminalCorePlugin,
};
use crate::server::{ServerSpawnConfig, TerminalServer};
use crate::session::SessionId;
use crate::shell::ShellSpec;
use nexus_plugins::PluginError;

use super::shared::{crate_err, exec_err, parse_args, poisoned, to_value};

impl TerminalCorePlugin {
    /// BL-142 Phase 1 — spawn a language kernel and register it as
    /// a REPL session. Reuses the existing `create_session` /
    /// `ShellSpec` machinery so PTY, scrollback, output bus, memory
    /// monitor all apply unchanged; the only delta is bookkeeping
    /// in `self.repls` so `repl_eval` / `repl_stop` can validate the
    /// target id.
    pub(crate) fn dispatch_repl_start(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ReplStartArgs = parse_args(args, "repl_start")?;
        let cfg = ServerSpawnConfig {
            // Surface the lang in the human-readable session name so
            // tools that list sessions (terminal sidebar, etc.) can
            // tell REPL sessions apart from regular shells without
            // needing to query the REPL map.
            name: Some(format!("repl:{}", a.lang)),
            shell: Some(ShellSpec {
                program: PathBuf::from(&a.program),
                args: a.args.clone(),
            }),
            working_dir: a.working_dir.map(PathBuf::from),
            env: a.env,
        };
        let id = self
            .server
            .lock()
            .map_err(poisoned)?
            .create_session(cfg)
            .map_err(crate_err)?;
        let info = ReplInfo {
            id: id.as_str().to_string(),
            lang: a.lang.clone(),
            program: a.program,
            args: a.args,
            started_at_ms: chrono::Utc::now().timestamp_millis(),
        };
        self.repls
            .lock()
            .map_err(poisoned)?
            .insert(id.clone(), info);
        to_value(
            &ReplStartResponse {
                id: id.as_str().to_string(),
                lang: a.lang,
            },
            "repl_start",
        )
    }

    /// BL-142 Phase 1 — send `code` to a registered REPL session's
    /// stdin. Output streams asynchronously on the existing
    /// `com.nexus.terminal.output.<session_id>` event topic; this
    /// dispatch returns as soon as the bytes are queued. Rejects
    /// when the id is unknown or refers to a non-REPL session.
    pub(crate) fn dispatch_repl_eval(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ReplEvalArgs = parse_args(args, "repl_eval")?;
        let id = SessionId::from_string(a.id);
        if !self.repls.lock().map_err(poisoned)?.contains_key(&id) {
            return Err(exec_err(format!(
                "repl_eval: session '{id}' is not a registered REPL \
                 (use repl_start to spawn one, or send_input to write \
                 to a regular terminal session)",
                id = id.as_str(),
            )));
        }
        self.server
            .lock()
            .map_err(poisoned)?
            .send_input(&id, &a.code)
            .map_err(crate_err)?;
        Ok(serde_json::Value::Null)
    }

    /// BL-142 Phase 1 — close a registered REPL session. Same effect
    /// as `close_session` but enforces the REPL identity (so a buggy
    /// shell binding can't accidentally close a regular terminal
    /// tab) and removes the REPL bookkeeping entry.
    pub(crate) fn dispatch_repl_stop(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SessionIdArgs = parse_args(args, "repl_stop")?;
        let id = SessionId::from_string(a.id);
        if self.repls.lock().map_err(poisoned)?.remove(&id).is_none() {
            return Err(exec_err(format!(
                "repl_stop: session '{id}' is not a registered REPL",
                id = id.as_str(),
            )));
        }
        self.server
            .lock()
            .map_err(poisoned)?
            .close_session(&id)
            .map_err(crate_err)?;
        if let Ok(mut em) = self.emitters.lock() {
            em.remove(&id);
        }
        Ok(serde_json::Value::Null)
    }

    /// BL-142 Phase 1 — snapshot every currently-registered REPL
    /// session for shell discovery / tests. Sorted by `started_at_ms`
    /// ascending so the output is stable across calls when nothing
    /// changes.
    pub(crate) fn dispatch_repl_list(&self) -> Result<serde_json::Value, PluginError> {
        let map = self.repls.lock().map_err(poisoned)?;
        let mut infos: Vec<ReplInfo> = map.values().cloned().collect();
        drop(map);
        infos.sort_by_key(|i| i.started_at_ms);
        to_value(&infos, "repl_list")
    }
}
