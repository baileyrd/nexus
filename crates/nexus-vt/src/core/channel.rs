//! No-op stand-in for rusty_term's L13 in-band side-channel.
//!
//! Upstream `rusty_term` carries an OSC-5379 JSON-RPC transport (the `l13`
//! feature) that exposes the terminal over MCP/LSP/ACP and pulls in the sibling
//! `rusty_lsp` crate plus `serde_json`. **Nexus does not adopt that in-band
//! transport** (RFC 0003): the terminal is surfaced through Nexus's own kernel
//! IPC + MCP server, not through an OSC channel embedded in the child's output
//! stream. The grid's OSC 133 command/exit-code *tracking* is kept (it is what
//! the agent-introspection facade reads); only the transport is neutralized.
//!
//! Keeping this module — with the same `pub(crate)` surface the core calls —
//! lets `grid.rs` / `osc.rs` / `parser.rs` stay byte-for-byte identical to
//! upstream while the private-OSC handling and the resource-change pushes become
//! dependency-free no-ops. This crate therefore links neither `rusty_lsp` nor
//! `serde_json`.

use super::grid::Grid;

/// Byte prefix the parser matches to route a private-OSC payload into the
/// channel. Set to a value real OSC sequences never carry, so the (no-op)
/// channel is never triggered.
pub(crate) const OSC_PREFIX: &[u8] = b"\x00nexus-vt-no-l13-channel\x00";

// Resource URIs referenced by the (now no-op) change-notification call sites in
// `grid.rs` / `osc.rs`. Values are immaterial — nothing reads them here.
pub(crate) const RES_CWD: &str = "terminal://cwd";
pub(crate) const RES_TITLE: &str = "terminal://title";
pub(crate) const RES_COMMAND: &str = "terminal://command";
pub(crate) const RES_DIMENSIONS: &str = "terminal://dimensions";

/// Per-session channel state held on the [`Grid`]. Empty in Nexus — there is no
/// in-band channel and therefore no resource subscriptions.
#[derive(Default)]
pub(crate) struct ChannelState;

/// Handle one private-OSC payload. No-op: Nexus has no in-band channel, so the
/// payload is dropped (this is the same "graceful degradation" an unaware
/// terminal gives an L13 client).
pub(crate) fn handle(_payload: &[u8], _g: &mut Grid, _responses: &mut Vec<u8>) {}

/// Push a resource-changed notification. No-op — Nexus reports terminal state
/// changes over the kernel event bus, not in-band (RFC 0003 Track A).
pub(crate) fn notify_resource_changed(_g: &Grid, _uri: &'static str, _responses: &mut Vec<u8>) {}

/// Push a typed `command_finished { exit }` notification. No-op — Nexus emits a
/// typed `CommandFinished` kernel event instead (RFC 0003 PR-4).
pub(crate) fn notify_command_finished(_g: &Grid, _exit: Option<i32>, _responses: &mut Vec<u8>) {}
