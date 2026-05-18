//! Canonical plugin identifiers used across the Nexus tree.
//!
//! Every Nexus plugin is referenced by a reverse-DNS string of the form
//! `com.nexus.<name>`. These constants exist so subsystem crates, frontends,
//! and tests can refer to a plugin without scattering string literals — keeping
//! Rust and the TypeScript shell aligned via ts-rs export.

/// Kernel itself — emits lifecycle / capability events.
pub const KERNEL: &str = "com.nexus.kernel";

/// Storage / file-as-truth subsystem.
pub const STORAGE: &str = "com.nexus.storage";

/// AI plugin — chat, RAG, ask handlers.
pub const AI: &str = "com.nexus.ai";

/// AI runtime — model invocation pipeline.
pub const AI_RUNTIME: &str = "com.nexus.ai.runtime";

/// Agent orchestration plugin.
pub const AGENT: &str = "com.nexus.agent";

/// Comments overlay plugin.
pub const COMMENTS: &str = "com.nexus.comments";

/// Editor plugin (CRDT + transactional file ops).
pub const EDITOR: &str = "com.nexus.editor";

/// Git plugin — branch / dirty / commit surface.
pub const GIT: &str = "com.nexus.git";

/// Link-preview metadata plugin.
pub const LINKPREVIEW: &str = "com.nexus.linkpreview";

/// MCP host (Model Context Protocol bridge).
pub const MCP: &str = "com.nexus.mcp.host";

/// LSP host.
pub const LSP: &str = "com.nexus.lsp";

/// DAP host.
pub const DAP: &str = "com.nexus.dap";

/// ACP host.
pub const ACP: &str = "com.nexus.acp";

/// Skills plugin — authored prompt templates.
pub const SKILLS: &str = "com.nexus.skills";

/// Templates plugin.
pub const TEMPLATES: &str = "com.nexus.templates";

/// Terminal plugin.
pub const TERMINAL: &str = "com.nexus.terminal";

/// Theme plugin.
pub const THEME: &str = "com.nexus.theme";

/// Workflow runner.
pub const WORKFLOW: &str = "com.nexus.workflow";

/// Database plugin.
pub const DATABASE: &str = "com.nexus.database";

/// Key-value store plugin.
pub const KV: &str = "com.nexus.kv";

/// Security / audit plugin.
pub const SECURITY: &str = "com.nexus.security";

/// Format codecs plugin.
pub const FORMATS: &str = "com.nexus.formats";

/// Notifications plugin (inbox + transports).
pub const NOTIFICATIONS: &str = "com.nexus.notifications";

/// Audio plugin.
pub const AUDIO: &str = "com.nexus.audio";

/// Collaboration plugin.
pub const COLLAB: &str = "com.nexus.collab";

/// CLI host (event source for command invocations).
pub const CLI: &str = "com.nexus.cli";

/// TUI host.
pub const TUI: &str = "com.nexus.tui";
