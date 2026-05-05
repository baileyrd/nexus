//! Native LLM function-calling for the AI plugin (BL-016).
//!
//! Phase 1 of BL-016 (the keystone Phase-1 backlog item) splits into
//! three sub-tasks; this module is sub-task 1 — the in-process
//! [`ToolRegistry`] + [`ToolExecutor`] surface plus the
//! `com.nexus.storage`-backed `read_file` / `write_file` built-ins.
//!
//! Sub-tasks 2 and 3 (Anthropic + `OpenAI` wire format, Ollama wire
//! format + dispatch loop) consume this surface; they live behind
//! `crate::tools::*` to keep one home for everything tool-call.
//!
//! See `docs/PRDs/BACKLOG.md` BL-016 and `docs/PRDs/12-ai-engine.md` §8
//! for context.

pub mod functions;
pub mod registry;

pub use functions::{
    git_log_schema, list_backlinks_schema, read_file_schema, register_extended_builtins,
    register_storage_builtins, search_forge_schema, write_file_schema, GitLogTool,
    ListBacklinksTool, ReadFileTool, SearchForgeTool, WriteFileTool,
};
pub use registry::{RegisteredTool, ToolError, ToolExecutor, ToolRegistry, ToolSchema};
