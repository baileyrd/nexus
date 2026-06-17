//! Session-local `todo` list — Phase 5.2 / RFC 0005.
//!
//! omp's `todo` is a *session cache only* scratchpad the model uses to plan and
//! check off multi-step work. Nexus hosts it the same way: the list lives in a
//! per-session [`TodoDispatcher`] that wraps the real tool dispatcher and
//! handles the `todo` tool **inline** — it never crosses an IPC boundary, so
//! there is no cross-session bleed and no re-entrancy. The state is ephemeral:
//! it lives for the duration of one `session_run` and is dropped with it.

use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{ToolCall, ToolDispatcher};

/// Plugin id the `todo` tool is advertised under (intercepted, never dispatched).
pub const TODO_TARGET: &str = "com.nexus.agent";
/// Command id of the `todo` tool.
pub const TODO_COMMAND: &str = "todo";

/// Lifecycle state of one todo item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoState {
    /// Not started.
    Pending,
    /// The single in-flight task (at most one at a time).
    InProgress,
    /// Finished.
    Completed,
    /// Given up / no longer relevant.
    Abandoned,
}

impl TodoState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
        }
    }
}

/// One task in the list.
#[derive(Debug, Clone)]
pub struct TodoItem {
    /// Stable 1-based id, assigned on insert.
    pub id: usize,
    /// Task description.
    pub text: String,
    /// Current state.
    pub state: TodoState,
}

/// An ordered task list with a single-active invariant.
#[derive(Debug, Default)]
pub struct TodoList {
    items: Vec<TodoItem>,
    next_id: usize,
}

impl TodoList {
    /// Replace every item with a fresh pending list built from `texts`.
    pub fn init(&mut self, texts: Vec<String>) {
        self.items.clear();
        self.next_id = 0;
        for text in texts {
            self.push(text);
        }
    }

    /// Append one pending item; returns its id.
    pub fn append(&mut self, text: String) -> usize {
        self.push(text)
    }

    fn push(&mut self, text: String) -> usize {
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(TodoItem {
            id,
            text,
            state: TodoState::Pending,
        });
        id
    }

    /// Mark `id` in-progress. Enforces the single-active invariant.
    ///
    /// # Errors
    /// Errors if `id` is unknown or another task is already in progress.
    pub fn start(&mut self, id: usize) -> Result<(), String> {
        if let Some(active) = self
            .items
            .iter()
            .find(|i| i.state == TodoState::InProgress)
        {
            if active.id != id {
                return Err(format!(
                    "task {} is already in progress; finish or drop it first",
                    active.id
                ));
            }
        }
        self.set_state(id, TodoState::InProgress)
    }

    /// Mark `id` completed.
    ///
    /// # Errors
    /// Errors if `id` is unknown.
    pub fn done(&mut self, id: usize) -> Result<(), String> {
        self.set_state(id, TodoState::Completed)
    }

    /// Mark `id` abandoned.
    ///
    /// # Errors
    /// Errors if `id` is unknown.
    pub fn drop_item(&mut self, id: usize) -> Result<(), String> {
        self.set_state(id, TodoState::Abandoned)
    }

    fn set_state(&mut self, id: usize, state: TodoState) -> Result<(), String> {
        let item = self
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| format!("no task with id {id}"))?;
        item.state = state;
        Ok(())
    }

    /// Render the list as a JSON value the model sees after every op.
    #[must_use]
    pub fn render(&self) -> Value {
        json!({
            "items": self
                .items
                .iter()
                .map(|i| json!({ "id": i.id, "text": i.text, "state": i.state.as_str() }))
                .collect::<Vec<_>>(),
        })
    }
}

/// Apply one `todo` tool call to `list`, returning the updated list view.
///
/// # Errors
/// Returns a message for a missing/invalid `op` or a failed mutation.
pub fn handle(list: &mut TodoList, args: &Value) -> Result<Value, String> {
    let op = args
        .get("op")
        .and_then(Value::as_str)
        .ok_or("todo: missing 'op'")?;
    match op {
        "init" => {
            let texts = args
                .get("items")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            list.init(texts);
        }
        "append" => {
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .ok_or("todo append: missing 'text'")?;
            list.append(text.to_string());
        }
        "start" => list.start(id_arg(args)?)?,
        "done" => list.done(id_arg(args)?)?,
        "drop" => list.drop_item(id_arg(args)?)?,
        "view" => {}
        other => return Err(format!("todo: unknown op '{other}'")),
    }
    Ok(list.render())
}

fn id_arg(args: &Value) -> Result<usize, String> {
    args.get("id")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| "todo: this op requires an integer 'id'".to_string())
}

/// A [`ToolDispatcher`] decorator that handles the session-local `todo` tool
/// inline and delegates every other call to `inner`.
pub struct TodoDispatcher<D> {
    inner: D,
    list: Mutex<TodoList>,
}

impl<D> TodoDispatcher<D> {
    /// Wrap `inner` with an empty per-session todo list.
    pub fn new(inner: D) -> Self {
        Self {
            inner,
            list: Mutex::new(TodoList::default()),
        }
    }
}

#[async_trait]
impl<D: ToolDispatcher> ToolDispatcher for TodoDispatcher<D> {
    async fn dispatch(&self, call: &ToolCall) -> Result<Value, String> {
        if call.target_plugin_id == TODO_TARGET && call.command_id == TODO_COMMAND {
            let mut list = self.list.lock().map_err(|_| "todo list poisoned".to_string())?;
            return handle(&mut list, &call.args);
        }
        self.inner.dispatch(call).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(v: &Value) -> Vec<String> {
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| format!("{}:{}", i["text"].as_str().unwrap(), i["state"].as_str().unwrap()))
            .collect()
    }

    #[test]
    fn init_append_and_view() {
        let mut list = TodoList::default();
        handle(&mut list, &json!({ "op": "init", "items": ["a", "b"] })).unwrap();
        let v = handle(&mut list, &json!({ "op": "append", "text": "c" })).unwrap();
        assert_eq!(texts(&v), ["a:pending", "b:pending", "c:pending"]);
    }

    #[test]
    fn lifecycle_transitions() {
        let mut list = TodoList::default();
        handle(&mut list, &json!({ "op": "init", "items": ["a", "b"] })).unwrap();
        handle(&mut list, &json!({ "op": "start", "id": 1 })).unwrap();
        let v = handle(&mut list, &json!({ "op": "done", "id": 1 })).unwrap();
        assert_eq!(v["items"][0]["state"], "completed");
        let v = handle(&mut list, &json!({ "op": "drop", "id": 2 })).unwrap();
        assert_eq!(v["items"][1]["state"], "abandoned");
    }

    #[test]
    fn single_active_invariant() {
        let mut list = TodoList::default();
        handle(&mut list, &json!({ "op": "init", "items": ["a", "b"] })).unwrap();
        handle(&mut list, &json!({ "op": "start", "id": 1 })).unwrap();
        let err = handle(&mut list, &json!({ "op": "start", "id": 2 })).unwrap_err();
        assert!(err.contains("already in progress"), "got: {err}");
        // Re-starting the active task is fine; finishing it frees the slot.
        handle(&mut list, &json!({ "op": "start", "id": 1 })).unwrap();
        handle(&mut list, &json!({ "op": "done", "id": 1 })).unwrap();
        handle(&mut list, &json!({ "op": "start", "id": 2 })).unwrap();
    }

    #[test]
    fn errors_on_unknown_id_and_op() {
        let mut list = TodoList::default();
        assert!(handle(&mut list, &json!({ "op": "done", "id": 9 })).is_err());
        assert!(handle(&mut list, &json!({ "op": "frobnicate" })).is_err());
        assert!(handle(&mut list, &json!({ "op": "start" })).is_err());
    }

    struct DenyDispatcher;
    #[async_trait]
    impl ToolDispatcher for DenyDispatcher {
        async fn dispatch(&self, _call: &ToolCall) -> Result<Value, String> {
            Err("delegated".to_string())
        }
    }

    #[tokio::test]
    async fn decorator_intercepts_todo_and_delegates_others() {
        let d = TodoDispatcher::new(DenyDispatcher);
        // todo is handled inline …
        let todo_call = ToolCall {
            target_plugin_id: TODO_TARGET.to_string(),
            command_id: TODO_COMMAND.to_string(),
            args: json!({ "op": "append", "text": "x" }),
        };
        let v = d.dispatch(&todo_call).await.unwrap();
        assert_eq!(v["items"][0]["text"], "x");
        // … everything else falls through to the inner dispatcher.
        let other = ToolCall {
            target_plugin_id: "com.nexus.storage".to_string(),
            command_id: "read_file".to_string(),
            args: json!({}),
        };
        assert_eq!(d.dispatch(&other).await.unwrap_err(), "delegated");
    }
}
