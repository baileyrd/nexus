//! I/O + search handlers — `send_input`, `send_raw_input`, `pump`,
//! `read_output`, `read_raw_since`, `search_output`,
//! `wait_for_pattern`, `cross_session_search`. Split out of
//! `core_plugin.rs` by SD-03 terminal chunk 3.

use std::time::Duration;

use crate::core_plugin::TerminalCorePlugin;
use crate::ipc::{
    CrossSessionSearchArgs, LoadTranscriptResult, PumpArgs, PumpResponse, ReadOutputArgs,
    ReadRawSinceArgs, ReadRawSinceResponse, SearchOutputArgs, SendInputArgs, SendRawInputArgs,
    SessionIdArgs, WaitForPatternArgs, WaitForPatternResponse,
};
use crate::server::TerminalServer;
use crate::session::SessionId;
use nexus_plugins::PluginError;

use super::shared::{crate_err, exec_err, parse_args, poisoned, to_value};

impl TerminalCorePlugin {
    pub(crate) fn dispatch_send_input(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SendInputArgs = parse_args(args, "send_input")?;
        let id = SessionId::from_string(a.id);
        self.server
            .lock()
            .map_err(poisoned)?
            .send_input(&id, &a.input)
            .map_err(crate_err)?;
        Ok(serde_json::Value::Null)
    }

    pub(crate) fn dispatch_send_raw_input(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SendRawInputArgs = parse_args(args, "send_raw_input")?;
        let id = SessionId::from_string(a.id);
        self.server
            .lock()
            .map_err(poisoned)?
            .send_raw_input(&id, &a.data)
            .map_err(crate_err)?;
        Ok(serde_json::Value::Null)
    }

    pub(crate) fn dispatch_pump(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: PumpArgs = parse_args(args, "pump")?;
        let id = SessionId::from_string(a.id);
        let timeout = Duration::from_millis(a.timeout_ms.unwrap_or(100));
        // Hold the server lock just long enough to drain + read the new
        // bytes since our internal cursor; release before publishing so
        // a slow subscriber can't back-pressure the next IPC dispatch.
        let (bytes, new_chunk) = {
            let mut server = self.server.lock().map_err(poisoned)?;
            let bytes = server.pump(&id, timeout).map_err(crate_err)?;
            let new_chunk = self.fetch_new_bytes(&server, &id);
            (bytes, new_chunk)
        };
        if let Some((next_cursor, data)) = new_chunk {
            self.publish_output(&id, next_cursor, data);
        }
        to_value(&PumpResponse { bytes }, "pump")
    }

    pub(crate) fn dispatch_read_output(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ReadOutputArgs = parse_args(args, "read_output")?;
        let id = SessionId::from_string(a.id);
        let lines = self
            .server
            .lock()
            .map_err(poisoned)?
            .read_output(&id, a.start, a.count)
            .map_err(crate_err)?;
        to_value(&lines, "read_output")
    }

    pub(crate) fn dispatch_read_raw_since(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: ReadRawSinceArgs = parse_args(args, "read_raw_since")?;
        let id = SessionId::from_string(a.id);
        let timeout = Duration::from_millis(a.timeout_ms.unwrap_or(30));
        // Same lock-discipline as `dispatch_pump`: drain inside the
        // server lock, derive the event bytes from the plugin's
        // independent cursor, then drop the lock before publishing.
        let (cursor, data, new_chunk) = {
            let mut server = self.server.lock().map_err(poisoned)?;
            let (cursor, data) = server
                .read_raw_since(&id, a.cursor, timeout)
                .map_err(crate_err)?;
            let new_chunk = self.fetch_new_bytes(&server, &id);
            (cursor, data, new_chunk)
        };
        if let Some((next_cursor, bytes)) = new_chunk {
            self.publish_output(&id, next_cursor, bytes);
        }
        to_value(&ReadRawSinceResponse { cursor, data }, "read_raw_since")
    }

    pub(crate) fn dispatch_search_output(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SearchOutputArgs = parse_args(args, "search_output")?;
        let id = SessionId::from_string(a.id);
        let hits = self
            .server
            .lock()
            .map_err(poisoned)?
            .search_output(&id, &a.query, a.is_regex)
            .map_err(crate_err)?;
        to_value(&hits, "search_output")
    }

    pub(crate) fn dispatch_wait_for_pattern(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: WaitForPatternArgs = parse_args(args, "wait_for_pattern")?;
        let id = SessionId::from_string(a.id);
        let timeout = Duration::from_millis(a.timeout_ms);
        let matched = self
            .server
            .lock()
            .map_err(poisoned)?
            .wait_for_pattern(&id, &a.pattern, a.is_regex, timeout)
            .map_err(crate_err)?;
        to_value(&WaitForPatternResponse { matched }, "wait_for_pattern")
    }

    pub(crate) fn dispatch_cross_session_search(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: CrossSessionSearchArgs = parse_args(args, "cross_session_search")?;
        let store = self.session_store.as_ref().ok_or_else(|| {
            exec_err("session store not attached (runtime built without a forge path)".into())
        })?;
        let limit = usize::try_from(a.limit.unwrap_or(100))
            .unwrap_or(100)
            .max(1);
        let session_ids_slice = a.session_ids.as_deref();
        let store = store.lock().map_err(poisoned)?;
        let hits = store
            .cross_session_search(&a.query, a.is_regex, session_ids_slice, a.since_ts, limit)
            .map_err(crate_err)?;
        to_value(&hits, "cross_session_search")
    }

    /// C53 (#406) — wires up `SqliteSessionStore::list_metadata`, which
    /// previously had zero production callers. Every closed session
    /// with persisted scrollback (see `close_session`) shows up here,
    /// newest-accessed first.
    pub(crate) fn dispatch_list_persisted_sessions(
        &self,
    ) -> Result<serde_json::Value, PluginError> {
        let store = self.session_store.as_ref().ok_or_else(|| {
            exec_err("session store not attached (runtime built without a forge path)".into())
        })?;
        let store = store.lock().map_err(poisoned)?;
        let sessions = store.list_metadata().map_err(crate_err)?;
        to_value(&sessions, "list_persisted_sessions")
    }

    /// C53 (#406) — wires up `SqliteSessionStore::load_scrollback`,
    /// which previously had zero production callers. ANSI-strips the
    /// stored bytes the same way `save_scrollback`'s FTS indexing
    /// does, so the transcript reads as plain text.
    pub(crate) fn dispatch_load_transcript(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: SessionIdArgs = parse_args(args, "load_transcript")?;
        let store = self.session_store.as_ref().ok_or_else(|| {
            exec_err("session store not attached (runtime built without a forge path)".into())
        })?;
        let store = store.lock().map_err(poisoned)?;
        let bytes = store.load_scrollback(&a.id).map_err(crate_err)?;
        let text = bytes.map(|b| crate::ansi::strip_ansi(&b));
        to_value(&LoadTranscriptResult { text }, "load_transcript")
    }
}
