//! `ToolRegistry`, [`ToolExecutor`], [`ToolSchema`] — the in-process
//! registry that lets the AI plugin advertise function-calling tools to a
//! provider and dispatch the model's tool-calls back.
//!
//! Phase split (see `BL-016` in `docs/PRDs/BACKLOG.md`): this module is
//! sub-task 1 — the core surface only. Provider wire-format support
//! (Anthropic / `OpenAI` `tools` array, Ollama tool-call format) lands in
//! sub-tasks 2 and 3 and consumes [`ToolRegistry::schemas`] +
//! [`ToolRegistry::execute`]. The streaming dispatch loop in
//! `core_plugin::handle_stream_chat` is also deferred.
//!
//! Spec: `docs/PRDs/12-ai-engine.md` §8.1. The shape follows the spec; the
//! one deliberate deviation is [`ToolError::NotFound`] carrying the tool
//! name (the PRD's enum has no field), which makes registry misses
//! debuggable without losing the round-trip.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// JSON-Schema-shaped description of a tool, surfaced to the model so it
/// knows the tool exists and what arguments to produce.
///
/// `input_schema` is a JSON Schema document; provider adapters reshape it
/// into the provider's native tool-call format (Anthropic's
/// `tools[].input_schema`, `OpenAI`'s `tools[].function.parameters`, Ollama's
/// `tools[].function.parameters`). The schema is not enforced inside
/// [`ToolRegistry`] — the executor receives the raw `serde_json::Value` the
/// model produced and is responsible for its own validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    /// Stable, unique tool name (e.g. `"read_file"`). Must match the
    /// key the tool is registered under or the model's tool-call won't
    /// resolve.
    pub name: String,
    /// One-paragraph description. The model uses this to decide *when*
    /// to call the tool, so it should describe behaviour, not
    /// implementation.
    pub description: String,
    /// JSON Schema for the tool's input. Top-level type is `object`.
    pub input_schema: serde_json::Value,
}

/// Errors that can occur during tool registration or execution.
#[derive(Debug, Error)]
pub enum ToolError {
    /// No tool registered under the given name. The string is the
    /// requested tool name, useful for surfacing back to the model
    /// ("you tried to call X which doesn't exist") and for logs.
    #[error("tool not found: {0}")]
    NotFound(String),

    /// The tool's executor returned an error. Free-form message;
    /// providers will surface it back to the model verbatim so the
    /// model can adjust its next call.
    #[error("tool execution failed: {0}")]
    ExecutionFailed(String),

    /// The model's tool-call args didn't match what the tool expects
    /// (missing field, wrong type, failed `serde_json::from_value`).
    /// Surfaced back to the model so it can retry with valid input.
    #[error("invalid tool input: {0}")]
    InvalidInput(String),
}

/// A tool the registry can dispatch. Implementations own whatever state
/// they need (a [`KernelPluginContext`] for IPC-backed tools, a database
/// handle for query tools, etc.) and turn the model's JSON args into a
/// string the model can read back.
///
/// The return type is `String` rather than `serde_json::Value` because
/// providers paste the result into a tool-result content block as text;
/// JSON results should be stringified by the executor (typically via
/// `serde_json::to_string_pretty`) so the model sees the same shape on
/// every provider.
///
/// [`KernelPluginContext`]: nexus_kernel::KernelPluginContext
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Run the tool with the model's JSON-shaped input. Errors should
    /// be classified into the right [`ToolError`] variant — providers
    /// surface `InvalidInput` and `ExecutionFailed` differently.
    ///
    /// # Errors
    /// Implementation-defined; see [`ToolError`].
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError>;
}

/// A registered tool — its schema (for the model) plus its executor
/// (for dispatch). Cheap to clone since the executor is `Arc`-shared.
#[derive(Clone)]
pub struct RegisteredTool {
    /// Schema advertised to the model.
    pub schema: ToolSchema,
    /// Executor that runs when the model calls this tool.
    pub executor: Arc<dyn ToolExecutor>,
}

/// In-process registry of tools the AI plugin can offer to providers.
///
/// The registry is intentionally not `Send + Sync` by interior mutability —
/// build it once during request setup, then read-only through the
/// streaming dispatch loop. Wrap in `Arc` if multiple handlers need to
/// share the same set.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, RegisteredTool>,
}

impl ToolRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool. Re-registering an existing name overwrites the
    /// previous entry (so callers can update an executor without a
    /// remove/insert dance).
    pub fn register(
        &mut self,
        name: impl Into<String>,
        schema: ToolSchema,
        executor: Arc<dyn ToolExecutor>,
    ) {
        let name = name.into();
        self.tools
            .insert(name, RegisteredTool { schema, executor });
    }

    /// Snapshot every registered schema, in registration order is
    /// **not** guaranteed (`HashMap` iteration order). Provider adapters
    /// don't depend on order; the model receives the array as-is.
    #[must_use]
    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|t| t.schema.clone()).collect()
    }

    /// Number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry has no tools.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Whether the registry has a tool with this name.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Execute the named tool with the given input.
    ///
    /// # Errors
    /// - [`ToolError::NotFound`] if no tool with `name` is registered.
    /// - Whatever the executor returns otherwise.
    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<String, ToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        tool.executor.execute(input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Test executor: records call count and either echoes the input
    /// back as a JSON string or returns a configured error.
    struct StubExecutor {
        calls: AtomicUsize,
        error: Option<ToolError>,
    }

    impl StubExecutor {
        fn echoing() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                error: None,
            }
        }

        fn failing(err: ToolError) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                error: Some(err),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl ToolExecutor for StubExecutor {
        async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &self.error {
                Some(ToolError::NotFound(s)) => Err(ToolError::NotFound(s.clone())),
                Some(ToolError::ExecutionFailed(s)) => Err(ToolError::ExecutionFailed(s.clone())),
                Some(ToolError::InvalidInput(s)) => Err(ToolError::InvalidInput(s.clone())),
                None => Ok(input.to_string()),
            }
        }
    }

    fn schema(name: &str) -> ToolSchema {
        ToolSchema {
            name: name.to_string(),
            description: format!("test tool {name}"),
            input_schema: serde_json::json!({ "type": "object" }),
        }
    }

    #[test]
    fn new_registry_is_empty() {
        let reg = ToolRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.schemas().is_empty());
    }

    #[test]
    fn register_inserts_and_lists_schema() {
        let mut reg = ToolRegistry::new();
        let exec = Arc::new(StubExecutor::echoing());
        reg.register("read_file", schema("read_file"), exec);

        assert_eq!(reg.len(), 1);
        assert!(reg.contains("read_file"));
        let schemas = reg.schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "read_file");
    }

    #[test]
    fn register_overwrites_existing_name() {
        let mut reg = ToolRegistry::new();
        reg.register(
            "search",
            schema("search"),
            Arc::new(StubExecutor::echoing()),
        );
        // Register a different schema under the same name; should replace.
        let mut new_schema = schema("search");
        new_schema.description = "updated".to_string();
        reg.register("search", new_schema, Arc::new(StubExecutor::echoing()));

        assert_eq!(reg.len(), 1);
        let schemas = reg.schemas();
        assert_eq!(schemas[0].description, "updated");
    }

    #[tokio::test]
    async fn execute_dispatches_to_named_tool() {
        let mut reg = ToolRegistry::new();
        let exec = Arc::new(StubExecutor::echoing());
        let exec_handle = Arc::clone(&exec);
        reg.register("echo", schema("echo"), exec);

        let out = reg
            .execute("echo", serde_json::json!({ "x": 1 }))
            .await
            .expect("execute");
        assert_eq!(out, r#"{"x":1}"#);
        assert_eq!(exec_handle.call_count(), 1);
    }

    #[tokio::test]
    async fn execute_unknown_tool_returns_not_found() {
        let reg = ToolRegistry::new();
        let err = reg
            .execute("missing", serde_json::json!({}))
            .await
            .expect_err("should error");
        match err {
            ToolError::NotFound(name) => assert_eq!(name, "missing"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_propagates_executor_error() {
        let mut reg = ToolRegistry::new();
        reg.register(
            "bad",
            schema("bad"),
            Arc::new(StubExecutor::failing(ToolError::ExecutionFailed(
                "boom".to_string(),
            ))),
        );

        let err = reg
            .execute("bad", serde_json::json!({}))
            .await
            .expect_err("should error");
        match err {
            ToolError::ExecutionFailed(msg) => assert_eq!(msg, "boom"),
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_propagates_invalid_input_error() {
        let mut reg = ToolRegistry::new();
        reg.register(
            "strict",
            schema("strict"),
            Arc::new(StubExecutor::failing(ToolError::InvalidInput(
                "missing 'path'".to_string(),
            ))),
        );

        let err = reg
            .execute("strict", serde_json::json!({}))
            .await
            .expect_err("should error");
        assert!(matches!(err, ToolError::InvalidInput(msg) if msg.contains("path")));
    }

    #[test]
    fn registry_clone_shares_executors() {
        let mut reg = ToolRegistry::new();
        let exec = Arc::new(StubExecutor::echoing());
        reg.register("t", schema("t"), Arc::clone(&exec) as Arc<dyn ToolExecutor>);

        let cloned = reg.clone();
        assert_eq!(cloned.len(), 1);
        assert!(cloned.contains("t"));
    }

    #[test]
    fn schema_round_trips_through_serde() {
        let s = schema("read_file");
        let json = serde_json::to_string(&s).expect("serialize");
        let back: ToolSchema = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.name, s.name);
        assert_eq!(back.description, s.description);
        assert_eq!(back.input_schema, s.input_schema);
    }
}
