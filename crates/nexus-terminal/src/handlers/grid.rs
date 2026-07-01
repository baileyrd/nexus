//! RFC 0003 Track A — server-side VT grid introspection handlers.
//!
//! Read-only reads of the per-session [`nexus_vt::Vt`] grid maintained by the
//! session manager: the visible screen, scrollback, working directory, cursor,
//! and the last command's exit code + captured output. These give agents /
//! CLI / TUI a structured view of a terminal that the frontend's own emulator
//! (xterm.js) keeps inside the webview. Surfaced as MCP terminal resources in
//! PR-7.

use crate::core_plugin::TerminalCorePlugin;
use crate::ipc::{
    GetCwdResponse, GetLastExitResponse, GetScreenResponse, GetScrollbackArgs,
    GetScrollbackResponse, GridCursor, SessionIdArgs,
};
use crate::session::SessionId;
use crate::TerminalError;
use nexus_plugins::PluginError;

use super::shared::{crate_err, parse_args, poisoned, to_value};

/// Default scrollback line cap when the caller omits `lines`.
const DEFAULT_SCROLLBACK_LINES: usize = 1000;

impl TerminalCorePlugin {
    pub(crate) fn dispatch_get_screen(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SessionIdArgs = parse_args(args, "get_screen")?;
        let id = SessionId::from_string(a.id);
        let server = self.server.lock().map_err(poisoned)?;
        let mgr = server.manager();
        let text = mgr.vt_screen(&id).ok_or_else(|| not_running(&id))?;
        let (col, row) = mgr.vt_cursor(&id).ok_or_else(|| not_running(&id))?;
        to_value(
            &GetScreenResponse {
                text,
                cursor: GridCursor { col, row },
            },
            "get_screen",
        )
    }

    pub(crate) fn dispatch_get_scrollback(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: GetScrollbackArgs = parse_args(args, "get_scrollback")?;
        let id = SessionId::from_string(a.id);
        let max = a.lines.unwrap_or(DEFAULT_SCROLLBACK_LINES);
        let server = self.server.lock().map_err(poisoned)?;
        let text = server
            .manager()
            .vt_scrollback(&id, max)
            .ok_or_else(|| not_running(&id))?;
        to_value(&GetScrollbackResponse { text }, "get_scrollback")
    }

    pub(crate) fn dispatch_get_cwd(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SessionIdArgs = parse_args(args, "get_cwd")?;
        let id = SessionId::from_string(a.id);
        let server = self.server.lock().map_err(poisoned)?;
        let cwd = server
            .manager()
            .vt_cwd(&id)
            .ok_or_else(|| not_running(&id))?;
        to_value(&GetCwdResponse { cwd }, "get_cwd")
    }

    pub(crate) fn dispatch_get_cursor(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SessionIdArgs = parse_args(args, "get_cursor")?;
        let id = SessionId::from_string(a.id);
        let server = self.server.lock().map_err(poisoned)?;
        let (col, row) = server
            .manager()
            .vt_cursor(&id)
            .ok_or_else(|| not_running(&id))?;
        to_value(&GridCursor { col, row }, "get_cursor")
    }

    pub(crate) fn dispatch_get_last_exit(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SessionIdArgs = parse_args(args, "get_last_exit")?;
        let id = SessionId::from_string(a.id);
        let server = self.server.lock().map_err(poisoned)?;
        let (exit_code, output) = server
            .manager()
            .vt_last_exit(&id)
            .ok_or_else(|| not_running(&id))?;
        to_value(&GetLastExitResponse { exit_code, output }, "get_last_exit")
    }
}

fn not_running(id: &SessionId) -> PluginError {
    crate_err(TerminalError::NotRunning(id.as_str().to_string()))
}
