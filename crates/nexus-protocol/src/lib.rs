//! Speech-act protocol for Nexus agent communication.
//!
//! This crate provides the **semantic layer** above the raw transport
//! (rmcp / MCP). Where rmcp says "here is a JSON payload," `nexus-protocol`
//! says "this is a *query* from the *user* to the *agent*, expecting a
//! *response* back."
//!
//! ## Design: speech acts
//!
//! Every message in the agent loop has a communicative intent — a
//! *speech act*. The seven acts defined here cover the full vocabulary
//! of user↔agent, agent↔tool, and agent↔agent communication:
//!
//! | Act | Intent |
//! |-----|--------|
//! | [`SpeechActKind::Request`] | "Please do X" |
//! | [`SpeechActKind::Inform`] | "X is true / here is the result" |
//! | [`SpeechActKind::Query`] | "What is X?" |
//! | [`SpeechActKind::Response`] | Answer to a Query or result of a Request |
//! | [`SpeechActKind::Propose`] | "I suggest doing X — approve?" |
//! | [`SpeechActKind::Confirm`] | Approval of a Propose |
//! | [`SpeechActKind::Decline`] | Rejection of a Propose or refusal |
//! | [`SpeechActKind::Observe`] | Recording an observation (no reply needed) |
//!
//! ## Channels
//!
//! Messages travel on one of four [`Channel`]s that determine the
//! expected response pattern and the capability gate that applies:
//!
//! | Channel | Participants |
//! |---------|-------------|
//! | `User` | Human ↔ Agent |
//! | `Tool` | Agent ↔ Tool/Capability |
//! | `Agent` | Agent ↔ Sub-agent (delegation) |
//! | `System` | Session lifecycle signals |
//!
//! ## Relationship to other crates
//!
//! - `nexus-context` (Move 5) records WHAT was said; `nexus-protocol`
//!   records WHY (the speech act kind).
//! - `nexus-ai-runtime` (Move 2/3) records capability-gated Proposals
//!   that map to [`SpeechActKind::Propose`].
//! - The rmcp / MCP transport in `nexus-mcp` is the wire layer below
//!   this; `nexus-protocol` types are transport-agnostic.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Message identity ─────────────────────────────────────────────────────────

/// Opaque identifier for a [`ProtocolMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(Uuid);

impl MessageId {
    /// Allocate a fresh random message id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Inner UUID value.
    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ─── Speech act ───────────────────────────────────────────────────────────────

/// The communicative intent of a [`ProtocolMessage`].
///
/// Classifies *why* a message was sent, not just *what* it contains.
/// The session worker uses the act kind to decide how to respond and
/// which capability gate to consult.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechActKind {
    /// "Please perform action X." The sender expects the receiver to
    /// act. User→agent requests trigger the perceive-reason-act loop;
    /// agent→tool requests go through the capability gate.
    Request,
    /// "X is true / here is the data." Conveying a fact, observation,
    /// or result without requesting further action.
    Inform,
    /// "What is X?" Seeking information from the receiver. The
    /// receiver is expected to reply with a `Response`.
    Query,
    /// The answer to a `Query` or the outcome of a `Request`.
    /// Tool results, model completions, and IPC replies all map here.
    Response,
    /// "I plan to do X — do you approve?" Emitted by the agent
    /// before a capability-gated action (maps to Move 3 `Proposal`).
    /// The receiver (user or supervisor) replies with `Confirm` or
    /// `Decline`.
    Propose,
    /// Approval of a `Propose`. The agent may now execute the action.
    Confirm,
    /// Rejection of a `Propose` or refusal of a `Request`. The sender
    /// will not perform the requested action; the `content` explains
    /// why.
    Decline,
    /// Recording an observation without expecting a reply. Used by the
    /// session worker for internal state transitions (e.g. "tool
    /// invocation succeeded") that are useful in the event log but do
    /// not drive the conversational turn.
    Observe,
}

// ─── Channel ─────────────────────────────────────────────────────────────────

/// The communication channel a message travels on.
///
/// The channel determines which capability gate applies and what kind
/// of response pattern is expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Human ↔ Agent: the primary user-facing channel. Agents receive
    /// `Request` / `Query` acts from users and reply with `Response` /
    /// `Propose` acts.
    User,
    /// Agent ↔ Tool/Capability: the agent calls a tool via IPC and
    /// the tool returns a result. Requires `IpcCall` capability.
    Tool,
    /// Agent ↔ Sub-agent: delegation from a parent session to a child
    /// session created via `CapabilityToken::attenuate`. Requires
    /// `AiRuntimeSubmit` capability.
    Agent,
    /// System-internal lifecycle signals (session start/end, budget
    /// warnings, supervisor heartbeats). Not surfaced to users or tools.
    System,
}

// ─── Participants ─────────────────────────────────────────────────────────────

/// One party in a [`ProtocolMessage`] exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Participant {
    /// The human user interacting with the system.
    User,
    /// An AI agent session.
    Agent {
        /// The session running this agent instance.
        session_id: Uuid,
    },
    /// A tool or capability provider.
    Tool {
        /// Reverse-DNS plugin id of the tool provider.
        plugin_id: String,
        /// Tool name within the plugin.
        tool_name: String,
    },
    /// The system / supervisor.
    System,
}

impl Participant {
    /// Construct a `Tool` participant.
    #[must_use]
    pub fn tool(plugin_id: impl Into<String>, tool_name: impl Into<String>) -> Self {
        Self::Tool {
            plugin_id: plugin_id.into(),
            tool_name: tool_name.into(),
        }
    }

    /// Construct an `Agent` participant.
    #[must_use]
    pub fn agent(session_id: Uuid) -> Self {
        Self::Agent { session_id }
    }
}

// ─── Message content ──────────────────────────────────────────────────────────

/// The payload carried by a [`ProtocolMessage`].
///
/// Content variants map to the speech act kinds they most naturally
/// carry, but the pairing is advisory — nothing prevents an `Inform`
/// from carrying `Structured` content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    /// Free-form natural language text.
    Text {
        /// The text content.
        text: String,
    },
    /// Structured data payload.
    Structured {
        /// The structured payload.
        data: serde_json::Value,
    },
    /// A tool invocation request (agent → tool `Request`).
    ToolCall {
        /// The tool to call.
        tool_name: String,
        /// Arguments to pass to the tool.
        arguments: serde_json::Value,
    },
    /// The result returned by a tool (tool → agent `Response`).
    ToolResult {
        /// Correlation id linking this result to the originating call.
        call_id: String,
        /// Whether the call succeeded.
        success: bool,
        /// Result content.
        content: String,
    },
    /// A capability-gated proposal (agent → user `Propose`).
    Proposal {
        /// Opaque proposal id (matches `ProposalId` in nexus-ai-runtime).
        proposal_id: Uuid,
        /// Human-readable description of the proposed action.
        action_description: String,
    },
}

impl MessageContent {
    /// Convenience constructor for a text message.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Convenience constructor for a successful tool result.
    #[must_use]
    pub fn tool_result_ok(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::ToolResult {
            call_id: call_id.into(),
            success: true,
            content: content.into(),
        }
    }

    /// Convenience constructor for a failed tool result.
    #[must_use]
    pub fn tool_result_err(call_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self::ToolResult {
            call_id: call_id.into(),
            success: false,
            content: error.into(),
        }
    }
}

// ─── Protocol message ─────────────────────────────────────────────────────────

/// A fully-typed protocol message carrying a speech act.
///
/// Every message has an `id`, a `channel`, a `sender`, a `receiver`,
/// an `act` kind, and a `content` payload. The optional `in_reply_to`
/// field links a `Response`, `Confirm`, or `Decline` to the message
/// it answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessage {
    /// Unique message identifier.
    pub id: MessageId,
    /// Communication channel this message travels on.
    pub channel: Channel,
    /// The speech act kind.
    pub act: SpeechActKind,
    /// Who sent the message.
    pub sender: Participant,
    /// Who the message is addressed to.
    pub receiver: Participant,
    /// The message payload.
    pub content: MessageContent,
    /// The message this is a reply to, if any.
    #[serde(default)]
    pub in_reply_to: Option<MessageId>,
    /// Session context for this message.
    #[serde(default)]
    pub session_id: Option<Uuid>,
    /// When the message was created.
    pub timestamp: DateTime<Utc>,
}

impl ProtocolMessage {
    /// Build a user `Request` addressed to an agent session.
    #[must_use]
    pub fn user_request(text: impl Into<String>, agent_session_id: Uuid) -> Self {
        Self {
            id: MessageId::new(),
            channel: Channel::User,
            act: SpeechActKind::Request,
            sender: Participant::User,
            receiver: Participant::agent(agent_session_id),
            content: MessageContent::text(text),
            in_reply_to: None,
            session_id: Some(agent_session_id),
            timestamp: Utc::now(),
        }
    }

    /// Build an agent `Response` addressed back to the user.
    #[must_use]
    pub fn agent_response(
        text: impl Into<String>,
        session_id: Uuid,
        in_reply_to: MessageId,
    ) -> Self {
        Self {
            id: MessageId::new(),
            channel: Channel::User,
            act: SpeechActKind::Response,
            sender: Participant::agent(session_id),
            receiver: Participant::User,
            content: MessageContent::text(text),
            in_reply_to: Some(in_reply_to),
            session_id: Some(session_id),
            timestamp: Utc::now(),
        }
    }

    /// Build an agent → tool `Request` (tool call).
    #[must_use]
    pub fn tool_call(
        session_id: Uuid,
        plugin_id: impl Into<String>,
        tool_name: impl Into<String>,
        arguments: serde_json::Value,
    ) -> Self {
        let plugin_id = plugin_id.into();
        let tool_name = tool_name.into();
        Self {
            id: MessageId::new(),
            channel: Channel::Tool,
            act: SpeechActKind::Request,
            sender: Participant::agent(session_id),
            receiver: Participant::tool(&plugin_id, &tool_name),
            content: MessageContent::ToolCall {
                tool_name,
                arguments,
            },
            in_reply_to: None,
            session_id: Some(session_id),
            timestamp: Utc::now(),
        }
    }

    /// Build a tool → agent `Response` (tool result).
    #[must_use]
    pub fn tool_result(
        session_id: Uuid,
        plugin_id: impl Into<String>,
        tool_name: impl Into<String>,
        call_id: impl Into<String>,
        content: impl Into<String>,
        success: bool,
        in_reply_to: MessageId,
    ) -> Self {
        let plugin_id = plugin_id.into();
        let tool_name = tool_name.into();
        Self {
            id: MessageId::new(),
            channel: Channel::Tool,
            act: SpeechActKind::Response,
            sender: Participant::tool(&plugin_id, &tool_name),
            receiver: Participant::agent(session_id),
            content: MessageContent::ToolResult {
                call_id: call_id.into(),
                success,
                content: content.into(),
            },
            in_reply_to: Some(in_reply_to),
            session_id: Some(session_id),
            timestamp: Utc::now(),
        }
    }

    /// Build an agent `Propose` message addressed to the user.
    #[must_use]
    pub fn agent_propose(
        session_id: Uuid,
        proposal_id: Uuid,
        action_description: impl Into<String>,
    ) -> Self {
        Self {
            id: MessageId::new(),
            channel: Channel::User,
            act: SpeechActKind::Propose,
            sender: Participant::agent(session_id),
            receiver: Participant::User,
            content: MessageContent::Proposal {
                proposal_id,
                action_description: action_description.into(),
            },
            in_reply_to: None,
            session_id: Some(session_id),
            timestamp: Utc::now(),
        }
    }

    /// Build a user `Confirm` in reply to a `Propose`.
    #[must_use]
    pub fn user_confirm(proposal_msg_id: MessageId) -> Self {
        Self {
            id: MessageId::new(),
            channel: Channel::User,
            act: SpeechActKind::Confirm,
            sender: Participant::User,
            receiver: Participant::System,
            content: MessageContent::text("confirmed"),
            in_reply_to: Some(proposal_msg_id),
            session_id: None,
            timestamp: Utc::now(),
        }
    }

    /// Build a user `Decline` in reply to a `Propose`.
    #[must_use]
    pub fn user_decline(proposal_msg_id: MessageId, reason: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            channel: Channel::User,
            act: SpeechActKind::Decline,
            sender: Participant::User,
            receiver: Participant::System,
            content: MessageContent::text(reason),
            in_reply_to: Some(proposal_msg_id),
            session_id: None,
            timestamp: Utc::now(),
        }
    }

    /// Build a system `Observe` message (no reply expected).
    #[must_use]
    pub fn observe(session_id: Uuid, observation: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            channel: Channel::System,
            act: SpeechActKind::Observe,
            sender: Participant::System,
            receiver: Participant::System,
            content: MessageContent::text(observation),
            in_reply_to: None,
            session_id: Some(session_id),
            timestamp: Utc::now(),
        }
    }

    /// `true` when this message expects a reply (Request, Query, Propose).
    #[must_use]
    pub fn expects_reply(&self) -> bool {
        matches!(
            self.act,
            SpeechActKind::Request | SpeechActKind::Query | SpeechActKind::Propose
        )
    }

    /// `true` when this message is a reply to another (Response, Confirm, Decline).
    #[must_use]
    pub fn is_reply(&self) -> bool {
        matches!(
            self.act,
            SpeechActKind::Response | SpeechActKind::Confirm | SpeechActKind::Decline
        )
    }
}

// ─── Conversation thread ──────────────────────────────────────────────────────

/// An ordered sequence of [`ProtocolMessage`]s forming a conversation.
///
/// The thread is the protocol-layer view of what `nexus-context`
/// calls "history". The context builder pulls `ContextEntry`s from
/// episodic memory; the `Conversation` is the richer typed view that
/// includes speech act metadata and reply chains.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Conversation {
    /// Messages in arrival order.
    pub messages: Vec<ProtocolMessage>,
}

impl Conversation {
    /// Create an empty conversation.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a message.
    pub fn push(&mut self, msg: ProtocolMessage) {
        self.messages.push(msg);
    }

    /// Find the message that `reply.in_reply_to` points to.
    #[must_use]
    pub fn find_replied_to(&self, reply: &ProtocolMessage) -> Option<&ProtocolMessage> {
        let parent_id = reply.in_reply_to.as_ref()?;
        self.messages.iter().find(|m| &m.id == parent_id)
    }

    /// All messages on a specific channel.
    #[must_use]
    pub fn on_channel(&self, channel: Channel) -> Vec<&ProtocolMessage> {
        self.messages
            .iter()
            .filter(|m| m.channel == channel)
            .collect()
    }

    /// Most recent N messages, newest last.
    #[must_use]
    pub fn tail(&self, n: usize) -> Vec<&ProtocolMessage> {
        let skip = self.messages.len().saturating_sub(n);
        self.messages[skip..].iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_session() -> Uuid {
        Uuid::new_v4()
    }

    #[test]
    fn user_request_has_correct_act_and_channel() {
        let sid = agent_session();
        let msg = ProtocolMessage::user_request("Hello", sid);
        assert_eq!(msg.act, SpeechActKind::Request);
        assert_eq!(msg.channel, Channel::User);
        assert_eq!(msg.sender, Participant::User);
        assert!(msg.expects_reply());
        assert!(!msg.is_reply());
    }

    #[test]
    fn agent_response_links_to_request() {
        let sid = agent_session();
        let req = ProtocolMessage::user_request("Help", sid);
        let resp = ProtocolMessage::agent_response("Sure!", sid, req.id);
        assert_eq!(resp.act, SpeechActKind::Response);
        assert_eq!(resp.in_reply_to, Some(req.id));
        assert!(resp.is_reply());
        assert!(!resp.expects_reply());
    }

    #[test]
    fn tool_call_and_result_round_trip() {
        let sid = agent_session();
        let call = ProtocolMessage::tool_call(
            sid,
            "com.nexus.storage",
            "read_file",
            serde_json::json!({ "path": "notes.md" }),
        );
        assert_eq!(call.act, SpeechActKind::Request);
        assert_eq!(call.channel, Channel::Tool);

        let result = ProtocolMessage::tool_result(
            sid,
            "com.nexus.storage",
            "read_file",
            "call-1",
            "# Notes",
            true,
            call.id,
        );
        assert_eq!(result.act, SpeechActKind::Response);
        assert_eq!(result.in_reply_to, Some(call.id));
    }

    #[test]
    fn propose_confirm_decline_flow() {
        let sid = agent_session();
        let proposal_id = Uuid::new_v4();
        let propose = ProtocolMessage::agent_propose(sid, proposal_id, "Write notes.md");
        assert_eq!(propose.act, SpeechActKind::Propose);
        assert!(propose.expects_reply());

        let confirm = ProtocolMessage::user_confirm(propose.id);
        assert_eq!(confirm.act, SpeechActKind::Confirm);
        assert_eq!(confirm.in_reply_to, Some(propose.id));

        let decline = ProtocolMessage::user_decline(propose.id, "not now");
        assert_eq!(decline.act, SpeechActKind::Decline);
    }

    #[test]
    fn observe_does_not_expect_reply() {
        let sid = agent_session();
        let obs = ProtocolMessage::observe(sid, "Tool call succeeded.");
        assert_eq!(obs.act, SpeechActKind::Observe);
        assert!(!obs.expects_reply());
        assert!(!obs.is_reply());
    }

    #[test]
    fn conversation_find_replied_to() {
        let sid = agent_session();
        let mut conv = Conversation::new();
        let req = ProtocolMessage::user_request("Hello", sid);
        let req_id = req.id;
        conv.push(req);
        let resp = ProtocolMessage::agent_response("Hi!", sid, req_id);
        conv.push(resp.clone());
        let parent = conv.find_replied_to(&resp).expect("parent found");
        assert_eq!(parent.id, req_id);
    }

    #[test]
    fn conversation_on_channel_filters() {
        let sid = agent_session();
        let mut conv = Conversation::new();
        conv.push(ProtocolMessage::user_request("q", sid));
        conv.push(ProtocolMessage::observe(sid, "obs"));
        assert_eq!(conv.on_channel(Channel::User).len(), 1);
        assert_eq!(conv.on_channel(Channel::System).len(), 1);
        assert_eq!(conv.on_channel(Channel::Tool).len(), 0);
    }

    #[test]
    fn conversation_tail_returns_newest_n() {
        let sid = agent_session();
        let mut conv = Conversation::new();
        for _ in 0..5 {
            conv.push(ProtocolMessage::user_request("msg", sid));
        }
        assert_eq!(conv.tail(3).len(), 3);
        assert_eq!(conv.tail(10).len(), 5);
    }

    #[test]
    fn message_content_text_constructor() {
        if let MessageContent::Text { text } = MessageContent::text("hello") {
            assert_eq!(text, "hello");
        } else {
            panic!("expected Text");
        }
    }

    #[test]
    fn tool_result_ok_and_err_constructors() {
        if let MessageContent::ToolResult {
            success, content, ..
        } = MessageContent::tool_result_ok("id1", "data")
        {
            assert!(success);
            assert_eq!(content, "data");
        }
        if let MessageContent::ToolResult {
            success, content, ..
        } = MessageContent::tool_result_err("id2", "boom")
        {
            assert!(!success);
            assert_eq!(content, "boom");
        }
    }

    #[test]
    fn message_id_display_is_uuid_string() {
        let id = MessageId::new();
        let s = format!("{id}");
        assert_eq!(s.len(), 36); // UUID canonical form
    }
}
