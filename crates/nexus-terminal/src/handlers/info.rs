//! Session-introspection handlers — `get_session_info`,
//! `list_sessions`. Split out of `core_plugin.rs` by SD-03 terminal
//! chunk 1.

use crate::core_plugin::{SessionIdArgs, TerminalCorePlugin};
use crate::server::TerminalServer;
use crate::session::SessionId;
use nexus_plugins::PluginError;

use super::shared::{crate_err, parse_args, poisoned, to_value};

impl TerminalCorePlugin {
    pub(crate) fn dispatch_get_session_info(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SessionIdArgs = parse_args(args, "get_session_info")?;
        let id = SessionId::from_string(a.id);
        let mut info = self
            .server
            .lock()
            .map_err(poisoned)?
            .get_session_info(&id)
            .map_err(crate_err)?;
        info.rss_bytes = self.cached_rss(id.as_str());
        to_value(&info, "get_session_info")
    }

    pub(crate) fn dispatch_list_sessions(
        &self,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let mut list = self.server.lock().map_err(poisoned)?.list_sessions();
        // BL-061 — layer the cached RSS onto every row so a single
        // list_sessions call gives the shell UI everything it needs
        // for its memory chip without N follow-up `get_session_info`
        // calls.
        if self.memory.is_some() {
            for info in &mut list {
                info.rss_bytes = self.cached_rss(&info.id);
            }
        }
        to_value(&list, "list_sessions")
    }
}
