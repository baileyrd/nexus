//! Cross-plugin IPC composition tests.
//!
//! These tests exercise flows where one plugin *calls* another through
//! the kernel IPC surface. Unit tests and single-plugin integration
//! tests cover handlers in isolation; this file catches the class of
//! bugs that only surface when two or more plugins interact — argument
//! marshalling across the IPC boundary, handler-id drift when a callee
//! plugin renumbers, path-contract mismatches (e.g. writer puts files
//! somewhere reader looks elsewhere), deadlocks under concurrent
//! dispatch, and the engine staying responsive after a cross-plugin
//! failure.
//!
//! Conventions — mirror the single-plugin files:
//! - hermetic scratch forge via [`tempfile::TempDir`];
//! - `build_cli_runtime` drives the kernel like `nexus` CLI does;
//! - IPC calls through `runtime.context.ipc_call(PLUGIN_ID, …)`.
//!
//! LLM-backed handlers (`agent::plan`, `agent::run`, `ai::ask`, …) are
//! deliberately avoided — they require a configured provider. Where a
//! flow is described in the task spec but would need an LLM or an
//! external MCP server, it's skipped with a `// SKIP:` comment
//! explaining why.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{EventFilter, NexusEvent, PluginContext};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

fn write_workflow(root: &std::path::Path, relpath: &str, body: &str) {
    let abs = root.join(".workflows").join(relpath);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(abs, body).unwrap();
}

fn write_skill(root: &std::path::Path, relpath: &str, body: &str) {
    let abs = root.join(".forge").join("skills").join(relpath);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(abs, body).unwrap();
}

const SKILL_TONE: &str = r#"---
name: Tone
id: tone
description: applies a tone
version: 1.0.0
author: test
created: 2026-04-23
applicable_contexts: [ai-chat]
triggers: ["tone check"]
parameters:
  - name: tone
    type: string
    default: friendly
---
Write in a {{ tone }} style.
"#;

// ─── 1. workflow → skills::render ───────────────────────────────────────────

#[tokio::test]
async fn workflow_ipc_step_calls_skills_render_and_substitutes_body() {
    // The `ipc` step dispatcher in nexus-workflow routes straight to
    // com.nexus.skills::render. This pins the JSON argument shape
    // (`id`, `values`) across the IPC boundary: if skills' parse is
    // ever tightened (e.g. values becomes a required object), workflow
    // authors see a stable failure here, not a silent drift.
    const WF: &str = r#"
[workflow]
name = "RenderTone"

[trigger]
type = "manual"

[[steps]]
name = "render"
type = "ipc"
target = "com.nexus.skills"
command = "render"
[steps.args]
id = "tone"
[steps.args.values]
tone = "terse"
"#;
    let forge = scratch_forge();
    write_skill(forge.path(), "tone.skill.md", SKILL_TONE);
    write_workflow(forge.path(), "render.workflow.toml", WF);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = runtime
        .context
        .ipc_call(
            "com.nexus.workflow",
            "run",
            serde_json::json!({ "name": "RenderTone" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("run ok");

    assert_eq!(v["success"], true, "got {v}");
    let body = v["steps"][0]["response"]["body"]
        .as_str()
        .expect("render returns {body: ...}");
    assert!(
        body.contains("terse"),
        "body should reflect workflow-supplied value; got {body:?}"
    );
}

// ─── 2. workflow → storage write, second step reads it back ─────────────────

#[tokio::test]
async fn workflow_two_ipc_steps_share_state_through_storage() {
    // Two sequential steps against the storage plugin. Step 1 writes,
    // step 2 reads. If the storage engine didn't make the write
    // visible to a subsequent read (e.g. atomic write landing in a
    // tmp dir), step 2 would fail. Proves the state-sharing contract
    // across the workflow → storage boundary.
    const WF: &str = r#"
[workflow]
name = "WriteThenRead"

[trigger]
type = "manual"

[[steps]]
name = "write"
type = "ipc"
target = "com.nexus.storage"
command = "write_file"
[steps.args]
path = "chain.txt"
bytes = [67, 72, 65, 73, 78]  # "CHAIN"

[[steps]]
name = "read"
type = "ipc"
target = "com.nexus.storage"
command = "read_file"
[steps.args]
path = "chain.txt"
"#;
    let forge = scratch_forge();
    write_workflow(forge.path(), "chain.workflow.toml", WF);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = runtime
        .context
        .ipc_call(
            "com.nexus.workflow",
            "run",
            serde_json::json!({ "name": "WriteThenRead" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("run ok");

    assert_eq!(v["success"], true, "got {v}");
    let read_bytes = v["steps"][1]["response"]["bytes"]
        .as_array()
        .expect("read step returns {bytes: [...]}");
    let content: Vec<u8> = read_bytes
        .iter()
        .map(|n| u8::try_from(n.as_u64().unwrap()).unwrap())
        .collect();
    assert_eq!(std::str::from_utf8(&content).unwrap(), "CHAIN");
}

// ─── 3. workflow error path — bad target plugin ─────────────────────────────

#[tokio::test]
async fn workflow_step_with_unknown_target_plugin_marks_failed_and_skips_rest() {
    // Step 1 targets a nonexistent plugin id. The dispatcher returns
    // an error; executor records `Failed`. Default `on_error = stop`
    // means step 2 lands as `Skipped`. The overall run reports
    // `success: false` but does NOT crash — critical for the UI which
    // relies on a stable shape.
    const WF: &str = r#"
[workflow]
name = "BadTarget"

[trigger]
type = "manual"

[[steps]]
name = "broken"
type = "ipc"
target = "com.nowhere.notaplugin"
command = "nope"
[steps.args]
x = 1

[[steps]]
name = "unreached"
type = "ipc"
target = "com.nexus.storage"
command = "file_exists"
[steps.args]
path = "whatever.txt"
"#;
    let forge = scratch_forge();
    write_workflow(forge.path(), "bad.workflow.toml", WF);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = runtime
        .context
        .ipc_call(
            "com.nexus.workflow",
            "run",
            serde_json::json!({ "name": "BadTarget" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("run returns a shape even on step failure");

    assert_eq!(v["success"], false, "step failure must mark run unsuccessful: {v}");
    assert_eq!(v["steps"][0]["status"], "failed");
    assert_eq!(v["steps"][1]["status"], "skipped");
    // Error message mentions the plugin-not-found reason somewhere.
    let err = v["steps"][0]["error"].as_str().unwrap_or_default();
    assert!(
        !err.is_empty(),
        "failed step must carry an error message; got {v}"
    );
}

#[tokio::test]
async fn workflow_engine_stays_responsive_after_a_failed_cross_plugin_step() {
    // Regression guard: a failing ipc step must not poison the
    // workflow registry or the kernel's per-plugin lock. We run a
    // workflow that fails, then immediately run a second workflow
    // that succeeds — if the mutex were poisoned or the context
    // dropped, the second call would error.
    const BAD: &str = r#"
[workflow]
name = "BadAgain"

[trigger]
type = "manual"

[[steps]]
name = "broken"
type = "ipc"
target = "com.nexus.storage"
command = "no_such_command"
"#;
    const GOOD: &str = r#"
[workflow]
name = "StillWorks"

[trigger]
type = "manual"

[[steps]]
name = "exists"
type = "ipc"
target = "com.nexus.storage"
command = "file_exists"
[steps.args]
path = "nothing.md"
"#;
    let forge = scratch_forge();
    write_workflow(forge.path(), "bad.workflow.toml", BAD);
    write_workflow(forge.path(), "good.workflow.toml", GOOD);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let bad = runtime
        .context
        .ipc_call(
            "com.nexus.workflow",
            "run",
            serde_json::json!({ "name": "BadAgain" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("bad run returns shape");
    assert_eq!(bad["success"], false);
    assert_eq!(bad["steps"][0]["status"], "failed");

    let good = runtime
        .context
        .ipc_call(
            "com.nexus.workflow",
            "run",
            serde_json::json!({ "name": "StillWorks" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("good run still works after a failed one");
    assert_eq!(good["success"], true, "engine must stay responsive: {good}");
    assert_eq!(good["steps"][0]["status"], "ok");
}

// ─── 4. agent → skills::render via preset plan ──────────────────────────────

#[tokio::test]
async fn agent_run_plan_dispatches_tool_call_into_skills_render() {
    let forge = scratch_forge();
    write_skill(forge.path(), "tone.skill.md", SKILL_TONE);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let plan = serde_json::json!({
        "id": "plan-render-1",
        "goal": "render tone",
        "steps": [{
            "id": "render",
            "description": "render the tone skill",
            "tool_call": {
                "target_plugin_id": "com.nexus.skills",
                "command_id": "render",
                "args": { "id": "tone", "values": { "tone": "crisp" } }
            }
        }]
    });
    let obs = runtime
        .context
        .ipc_call(
            "com.nexus.agent",
            "run_plan",
            serde_json::json!({ "plan": plan }),
            CALL_TIMEOUT,
        )
        .await
        .expect("run_plan ok");

    assert_eq!(obs["success"], true, "got {obs}");
    let body = obs["steps"][0]["response"]["body"]
        .as_str()
        .expect("agent forwards skills::render response");
    assert!(
        body.contains("crisp"),
        "agent → skills substitution must propagate; got {body:?}"
    );
}

// ─── 5. agent → workflow::run via preset plan (3-hop: agent → workflow →
// storage) ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn agent_run_plan_can_invoke_workflow_run_which_writes_through_storage() {
    const WF: &str = r#"
[workflow]
name = "ThreeHop"

[trigger]
type = "manual"

[[steps]]
name = "write"
type = "ipc"
target = "com.nexus.storage"
command = "write_file"
[steps.args]
path = "three-hop.txt"
bytes = [79, 75]  # "OK"
"#;
    let forge = scratch_forge();
    write_workflow(forge.path(), "three.workflow.toml", WF);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let plan = serde_json::json!({
        "id": "plan-threehop-1",
        "goal": "exercise 3-hop IPC",
        "steps": [{
            "id": "run-wf",
            "description": "run the ThreeHop workflow",
            "tool_call": {
                "target_plugin_id": "com.nexus.workflow",
                "command_id": "run",
                "args": { "name": "ThreeHop" }
            }
        }]
    });
    let obs = runtime
        .context
        .ipc_call(
            "com.nexus.agent",
            "run_plan",
            serde_json::json!({ "plan": plan }),
            CALL_TIMEOUT,
        )
        .await
        .expect("run_plan ok");

    assert_eq!(obs["success"], true, "3-hop plan: {obs}");
    // Agent step response is the workflow-run shape.
    let nested = &obs["steps"][0]["response"];
    assert_eq!(nested["workflow_name"], "ThreeHop");
    assert_eq!(nested["success"], true);

    // Verify the storage side effect landed by reading through the
    // storage plugin directly — closes the loop.
    let read = runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "read_file",
            serde_json::json!({ "path": "three-hop.txt" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("read_file ok");
    let bytes = read["bytes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| u8::try_from(n.as_u64().unwrap()).unwrap())
        .collect::<Vec<u8>>();
    assert_eq!(std::str::from_utf8(&bytes).unwrap(), "OK");
}

// ─── 6. agent events + history persistence through storage ─────────────────

#[tokio::test]
async fn agent_run_plan_events_fire_and_history_lands_where_storage_reads_it() {
    // Cross-plugin invariant: agent's save_history() routes history
    // bytes through com.nexus.storage::write_file. The history_list
    // handler then reads them back via ctx.read_file/list_files. If
    // the path contract ever drifts (e.g. history written to
    // `.forge/agent/history/` but list_files looked under
    // `agent/history/`), history_list would return empty. This test
    // drives the full loop end-to-end and subscribes to the kernel
    // bus to pin the event-emission contract at the same time.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let mut sub = runtime
        .context
        .subscribe(EventFilter::CustomPrefix("com.nexus.agent.".into()));

    let plan = serde_json::json!({
        "id": "plan-histxplug-1",
        "goal": "history through storage",
        "steps": [{
            "id": "mark",
            "description": "write a marker",
            "tool_call": {
                "target_plugin_id": "com.nexus.storage",
                "command_id": "write_file",
                "args": { "path": "hx.txt", "bytes": [72, 88] }
            }
        }]
    });
    let obs = runtime
        .context
        .ipc_call(
            "com.nexus.agent",
            "run_plan",
            serde_json::json!({ "plan": plan }),
            CALL_TIMEOUT,
        )
        .await
        .expect("run_plan ok");
    assert_eq!(obs["success"], true);

    // Drain the bus events the agent emitted.
    let mut seen: Vec<String> = Vec::new();
    while let Some(ev) = sub.try_recv().expect("try_recv ok") {
        if let NexusEvent::Custom { type_id, .. } = &ev.event {
            seen.push(type_id.clone());
        }
    }
    assert_eq!(
        seen.first().map(String::as_str),
        Some("com.nexus.agent.run_start"),
        "first event must be run_start; got {seen:?}"
    );
    assert_eq!(
        seen.last().map(String::as_str),
        Some("com.nexus.agent.run_done"),
        "last event must be run_done; got {seen:?}"
    );

    // history_list crosses back through storage::list_files.
    let list = runtime
        .context
        .ipc_call(
            "com.nexus.agent",
            "history_list",
            serde_json::json!({}),
            CALL_TIMEOUT,
        )
        .await
        .unwrap();
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|e| e["plan_id"] == "plan-histxplug-1"),
        "history persisted via storage must be visible through history_list; got {list}"
    );

    // And the exact file exists where storage::read_file can reach it.
    let read = runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "read_file",
            serde_json::json!({ "path": ".forge/agent/history/plan-histxplug-1.json" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("history file must be at the path storage reads");
    let bytes_len = read["bytes"].as_array().unwrap().len();
    assert!(bytes_len > 0, "history file should be non-empty");
}

// ─── 7. ai::session_save → storage::read_file path contract ─────────────────

#[tokio::test]
async fn ai_session_save_lands_at_path_storage_read_file_can_reach() {
    // ai plugin writes through com.nexus.storage::write_file but the
    // filename computation (`<forge>/.forge/chat/sessions/<id>.json`)
    // lives in ai. If that ever drifts from what callers expect,
    // session_list would miss the save. This test asserts the
    // write → direct-storage-read round-trip succeeds at the exact
    // forge-relative path.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    runtime
        .context
        .ipc_call(
            "com.nexus.ai",
            "session_save",
            serde_json::json!({
                "id": "xplug-sess-1",
                "title": "CrossCheck",
                "messages": [{ "role": "user", "content": "hi" }],
                "updated_at": "2026-04-23T00:00:00Z",
            }),
            CALL_TIMEOUT,
        )
        .await
        .expect("session_save ok");

    let read = runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "read_file",
            serde_json::json!({ "path": ".forge/chat/sessions/xplug-sess-1.json" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("session file must be reachable via storage::read_file");
    let bytes = read["bytes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| u8::try_from(n.as_u64().unwrap()).unwrap())
        .collect::<Vec<u8>>();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("valid json");
    assert_eq!(json["id"], "xplug-sess-1");
    assert_eq!(json["title"], "CrossCheck");
}

// ─── 8. workflow::reload picks up a file staged via storage::write_file ─────

#[tokio::test]
async fn workflow_reload_sees_workflow_written_through_storage_plugin() {
    // Cross-plugin: we write a .workflow.toml via com.nexus.storage,
    // then call com.nexus.workflow::reload, then com.nexus.workflow::run.
    // Pins that workflow's discovery scan reads the same tree storage
    // writes into.
    const WF: &str = r#"
[workflow]
name = "LateBloomer"

[trigger]
type = "manual"

[[steps]]
name = "ping"
type = "noop"
"#;
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Stage the file through storage — same engine path the rest of
    // Nexus uses to author docs.
    runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "write_file",
            serde_json::json!({
                "path": ".workflows/late.workflow.toml",
                "bytes": WF.as_bytes().to_vec(),
            }),
            CALL_TIMEOUT,
        )
        .await
        .expect("storage write ok");

    let reload = runtime
        .context
        .ipc_call(
            "com.nexus.workflow",
            "reload",
            serde_json::json!({}),
            CALL_TIMEOUT,
        )
        .await
        .unwrap();
    assert!(
        reload["loaded"].as_u64().unwrap_or(0) >= 1,
        "reload should pick up the storage-written workflow; got {reload}"
    );

    let run = runtime
        .context
        .ipc_call(
            "com.nexus.workflow",
            "run",
            serde_json::json!({ "name": "LateBloomer" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("run ok");
    assert_eq!(run["success"], true);
    assert_eq!(run["steps"][0]["status"], "ok");
}

// ─── 9. concurrency — parallel storage writes must not deadlock ─────────────

#[tokio::test]
async fn concurrent_storage_writes_from_different_tasks_both_land() {
    // Two ipc_calls in flight simultaneously against the same handler.
    // The kernel serialises per-plugin dispatch, but the boundary
    // must not deadlock (e.g. if per-plugin lock were held across an
    // .await into another plugin). A tokio::join! completes only
    // when both futures resolve.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let ctx = &runtime.context;
    let a = ctx.ipc_call(
        "com.nexus.storage",
        "write_file",
        serde_json::json!({ "path": "c/a.txt", "bytes": [65] }),
        CALL_TIMEOUT,
    );
    let b = ctx.ipc_call(
        "com.nexus.storage",
        "write_file",
        serde_json::json!({ "path": "c/b.txt", "bytes": [66] }),
        CALL_TIMEOUT,
    );
    let (ra, rb) = tokio::join!(a, b);
    ra.expect("write a ok");
    rb.expect("write b ok");

    // Both files land.
    let ea = runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "file_exists",
            serde_json::json!({ "path": "c/a.txt" }),
            CALL_TIMEOUT,
        )
        .await
        .unwrap();
    let eb = runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "file_exists",
            serde_json::json!({ "path": "c/b.txt" }),
            CALL_TIMEOUT,
        )
        .await
        .unwrap();
    assert_eq!(ea["exists"], true, "got {ea}");
    assert_eq!(eb["exists"], true, "got {eb}");
}

// ─── 10. concurrency — workflow::run + skills::render in parallel ───────────

#[tokio::test]
async fn concurrent_workflow_run_and_skills_render_both_succeed() {
    // Two different plugins, called concurrently. Workflow runs an
    // ipc step into storage while skills::render runs purely in-memory.
    // Guards against cross-plugin deadlock: if workflow held its
    // registry lock across the await on storage AND skills shared any
    // lock (it doesn't today, but this test would catch a regression),
    // one side would block forever.
    const WF: &str = r#"
[workflow]
name = "Concur"

[trigger]
type = "manual"

[[steps]]
name = "ping"
type = "ipc"
target = "com.nexus.storage"
command = "file_exists"
[steps.args]
path = "any.txt"
"#;
    let forge = scratch_forge();
    write_skill(forge.path(), "tone.skill.md", SKILL_TONE);
    write_workflow(forge.path(), "concur.workflow.toml", WF);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let ctx = &runtime.context;
    let wf_fut = ctx.ipc_call(
        "com.nexus.workflow",
        "run",
        serde_json::json!({ "name": "Concur" }),
        CALL_TIMEOUT,
    );
    let sk_fut = ctx.ipc_call(
        "com.nexus.skills",
        "render",
        serde_json::json!({ "id": "tone", "values": { "tone": "zippy" } }),
        CALL_TIMEOUT,
    );
    let (wf, sk) = tokio::join!(wf_fut, sk_fut);
    let wf = wf.expect("workflow run ok");
    let sk = sk.expect("skills render ok");
    assert_eq!(wf["success"], true);
    assert!(sk["body"].as_str().unwrap().contains("zippy"));
}

// ─── 11. workflow variable interpolation threads through to storage ────────

#[tokio::test]
async fn workflow_interpolated_variable_reaches_storage_path_arg() {
    // Caller passes `variables.trigger.path`; workflow substitutes
    // into `steps.args.path`; storage::read_file receives the final
    // string. Exercises the full variable-pipeline hop across the
    // workflow ↔ storage boundary, not just the workflow internals.
    const WF: &str = r#"
[workflow]
name = "InterpStorage"

[trigger]
type = "manual"

[[steps]]
name = "read"
type = "ipc"
target = "com.nexus.storage"
command = "file_exists"
[steps.args]
path = "${trigger.path}"
"#;
    let forge = scratch_forge();
    write_workflow(forge.path(), "interp.workflow.toml", WF);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "write_file",
            serde_json::json!({ "path": "interp/seeded.md", "bytes": b"S".to_vec() }),
            CALL_TIMEOUT,
        )
        .await
        .expect("seed");

    let v = runtime
        .context
        .ipc_call(
            "com.nexus.workflow",
            "run",
            serde_json::json!({
                "name": "InterpStorage",
                "variables": { "trigger": { "path": "interp/seeded.md" } }
            }),
            CALL_TIMEOUT,
        )
        .await
        .expect("run ok");
    assert_eq!(v["success"], true, "got {v}");
    assert_eq!(v["steps"][0]["response"]["exists"], true);
}

// ─── 12. agent run_plan can invoke ai::vectorstore_count, which itself
// routes into storage — a 3-hop dispatch that requires no LLM provider. ────

#[tokio::test]
async fn agent_run_plan_invokes_ai_vectorstore_count_which_routes_through_storage() {
    // agent → ai::vectorstore_count → storage::vectorstore_count. On
    // a fresh forge the vector store is empty. Catches bugs where
    // the ai plugin's wired KernelPluginContext loses its handle
    // between handlers (the sync `config` handler works with no
    // context, but `vectorstore_count` truly needs it).
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let plan = serde_json::json!({
        "id": "plan-vec-1",
        "goal": "count vectors",
        "steps": [{
            "id": "count",
            "description": "ask ai for vector count",
            "tool_call": {
                "target_plugin_id": "com.nexus.ai",
                "command_id": "vectorstore_count",
                "args": {}
            }
        }]
    });
    let obs = runtime
        .context
        .ipc_call(
            "com.nexus.agent",
            "run_plan",
            serde_json::json!({ "plan": plan }),
            CALL_TIMEOUT,
        )
        .await
        .expect("run_plan ok");

    assert_eq!(obs["success"], true, "got {obs}");
    assert_eq!(
        obs["steps"][0]["response"]["count"].as_u64(),
        Some(0),
        "fresh forge: zero vectors visible through agent → ai → storage"
    );
}

// ─── SKIPPED flows from the task spec ───────────────────────────────────────
//
// - workflow → agent::plan: skipped. `plan` goes through `ai::stream_chat`
//   which requires a live LLM provider. The 3-hop shape is covered by the
//   `agent → workflow → storage` test above (same IPC substrate, opposite
//   direction).
// - agent → skills trigger-body in system prompt: skipped. Requires
//   `agent::plan`, i.e. an LLM. The prompt-assembly path is unit-tested in
//   nexus-agent itself.
// - agent → mcp discovery: skipped for the same reason. mcp_ipc.rs covers
//   `list_servers` on empty-config; the agent's append_mcp_hint is called
//   from the LLM-gated plan path.
// - cron-trigger fires workflow: skipped. The minimal cron field is one
//   minute; waiting that long in CI is unacceptable. file_event trigger
//   already exercises the trigger-engine → bus → run loop in workflow_ipc.rs.
