//! Phase 4 WI-36 — JSON Schema emission harness for the pilot IPC types.
//!
//! Iterates the 5 pilot handlers' arg + return types and writes their
//! JSON Schema into `crates/nexus-bootstrap/schemas/ipc/`. The emitted
//! files are committed to the tree (same convention as Phase 1's
//! generated TS bindings); CI drift-check fails if running this harness
//! produces a git diff.
//!
//! Run with: `cargo test -p nexus-bootstrap --test ipc_schema_emit --features ts-export`.
//!
//! Under the default feature set the harness is a no-op so `cargo test
//! --workspace` doesn't need `schemars` on its classpath.

#![cfg(feature = "ts-export")]

use std::fs;
use std::path::PathBuf;

use schemars::{schema_for, JsonSchema};

use nexus_ai::ipc::{
    AiActivityListArgs, AiActivityListResult, AiStreamAskArgs, AiStreamAskMessage,
    AiStreamAskResult, AiStreamAskRole, AiStreamAskSource, AiStreamChatArgs, AiStreamChatMode,
    AiToolPolicy,
};
// FU-13 — RAG response shape (BL-038). TS bindings shipped already;
// emitting JSON Schema lets MCP / external tools consume the same
// contract over the wire.
use nexus_ai::{
    activity_log::{ActivityEntry, ActivityOutcome, ActivitySurface, ActivityToolCall},
    Citation, RagResponse,
};
use nexus_storage::ipc::{
    StorageListDirArgs, StorageListDirEntry, StorageListDirResult, StorageNoteAppendArgs,
    StorageNoteAppendResult, StorageReadFileArgs, StorageReadFileResult, StorageSearchArgs,
    StorageSearchHit, StorageSearchResult, StorageWriteFileArgs, StorageWriteFileResult,
};
// Audit-2026-05-01 P1-3 (#113): linkpreview is the first subsystem
// brought into the schema generator outside the original storage / ai
// pilot.
use nexus_linkpreview::core_plugin::FetchArgs as LinkPreviewFetchArgs;
use nexus_linkpreview::LinkPreview;
// nexus-git uses a wire-mirror module — handlers emit ad-hoc
// `serde_json::json!` and the impl types in `nexus_git::types`
// don't even derive `Serialize`.
use nexus_git::ipc::{
    GitBranch, GitCommitArgs, GitCommitReply, GitDiffHunk, GitDiffLine, GitLogArgs, GitLogEntry,
    GitOk, GitPathArgs, GitStatusReply,
};

/// Relative path under `crates/nexus-bootstrap/schemas/ipc/`. Emits
/// `<plugin>_<command>_<suffix>.json` so sibling types for the same
/// handler (args/result/hit/…) land next to each other alphabetically.
fn write_schema<T: JsonSchema>(handler_slug: &str, role: &str) {
    let schema = schema_for!(T);
    let pretty = serde_json::to_string_pretty(&schema)
        .expect("schema serializes to JSON")
        + "\n";
    let out = out_dir().join(format!("{handler_slug}_{role}.json"));
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).expect("mkdir -p schemas/ipc");
    }
    fs::write(&out, pretty).unwrap_or_else(|e| panic!("write {}: {e}", out.display()));
    println!("wrote {}", out.display());
}

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join("ipc")
}

#[test]
fn emit_pilot_ipc_schemas() {
    // ── com.nexus.storage::search ────────────────────────────────────────
    write_schema::<StorageSearchArgs>("com_nexus_storage__search", "args");
    write_schema::<StorageSearchHit>("com_nexus_storage__search", "hit");
    write_schema::<StorageSearchResult>("com_nexus_storage__search", "result");

    // ── com.nexus.storage::read_file ─────────────────────────────────────
    write_schema::<StorageReadFileArgs>("com_nexus_storage__read_file", "args");
    write_schema::<StorageReadFileResult>("com_nexus_storage__read_file", "result");

    // ── com.nexus.storage::write_file ────────────────────────────────────
    write_schema::<StorageWriteFileArgs>("com_nexus_storage__write_file", "args");
    write_schema::<StorageWriteFileResult>("com_nexus_storage__write_file", "result");

    // ── com.nexus.storage::note_append (BL-043) ──────────────────────────
    write_schema::<StorageNoteAppendArgs>("com_nexus_storage__note_append", "args");
    write_schema::<StorageNoteAppendResult>("com_nexus_storage__note_append", "result");

    // ── com.nexus.storage::list_dir ──────────────────────────────────────
    write_schema::<StorageListDirArgs>("com_nexus_storage__list_dir", "args");
    write_schema::<StorageListDirEntry>("com_nexus_storage__list_dir", "entry");
    write_schema::<StorageListDirResult>("com_nexus_storage__list_dir", "result");

    // ── com.nexus.ai::stream_ask ─────────────────────────────────────────
    write_schema::<AiStreamAskArgs>("com_nexus_ai__stream_ask", "args");
    write_schema::<AiStreamAskMessage>("com_nexus_ai__stream_ask", "message");
    write_schema::<AiStreamAskRole>("com_nexus_ai__stream_ask", "role");
    write_schema::<AiStreamAskSource>("com_nexus_ai__stream_ask", "source");
    write_schema::<AiStreamAskResult>("com_nexus_ai__stream_ask", "result");

    // ── com.nexus.ai::stream_chat ────────────────────────────────────────
    // Reuses `AiStreamAskMessage` / `AiStreamAskRole` for the messages
    // array; only the args envelope + mode/tool-policy enums are
    // stream_chat-specific. BL-010/011/034 consume these.
    write_schema::<AiStreamChatArgs>("com_nexus_ai__stream_chat", "args");
    write_schema::<AiStreamChatMode>("com_nexus_ai__stream_chat", "mode");
    write_schema::<AiToolPolicy>("com_nexus_ai__stream_chat", "tool_policy");

    // ── com.nexus.ai::ask (BL-038 RAG response) ──────────────────────────
    // The MCP surface re-uses this shape to expose RAG answers + their
    // citation list; emitting the JSON Schema keeps MCP-side decoders
    // honest as `Citation` evolves.
    write_schema::<Citation>("com_nexus_ai__ask", "citation");
    write_schema::<RagResponse>("com_nexus_ai__ask", "result");

    // ── com.nexus.ai::activity_list (BL-037) ─────────────────────────────
    // Per-forge AI activity timeline. The shell pane consumes
    // `ActivityEntry` directly; MCP / external tools can drive
    // `activity_list` against the same shape.
    write_schema::<AiActivityListArgs>("com_nexus_ai__activity_list", "args");
    write_schema::<AiActivityListResult>("com_nexus_ai__activity_list", "result");
    write_schema::<ActivityEntry>("com_nexus_ai__activity_list", "entry");
    write_schema::<ActivitySurface>("com_nexus_ai__activity_list", "surface");
    write_schema::<ActivityOutcome>("com_nexus_ai__activity_list", "outcome");
    write_schema::<ActivityToolCall>("com_nexus_ai__activity_list", "tool_call");

    // ── com.nexus.linkpreview::fetch (P1-3 first roll-out) ───────────────
    // The shell's canvas link-node overlay calls `fetch` with the URL
    // and renders the returned [`LinkPreview`]. Both the args and the
    // reply are simple (URL string in, optional metadata out), making
    // this an ideal first pilot for bringing the remaining subsystems
    // into the schema generator (audit-2026-05-01 P1-3, issue #113).
    write_schema::<LinkPreviewFetchArgs>("com_nexus_linkpreview__fetch", "args");
    write_schema::<LinkPreview>("com_nexus_linkpreview__fetch", "result");

    // ── com.nexus.git (P1-3 #113) ────────────────────────────────────────
    // Wire-mirror types — impl emits ad-hoc `serde_json::json!`.
    write_schema::<GitStatusReply>("com_nexus_git__status", "reply");
    write_schema::<GitLogArgs>("com_nexus_git__log", "args");
    write_schema::<GitLogEntry>("com_nexus_git__log", "entry");
    write_schema::<GitBranch>("com_nexus_git__branches", "entry");
    write_schema::<GitPathArgs>("com_nexus_git", "path_args");
    write_schema::<GitDiffHunk>("com_nexus_git__diff_file", "hunk");
    write_schema::<GitDiffLine>("com_nexus_git__diff_file", "line");
    write_schema::<GitCommitArgs>("com_nexus_git__commit", "args");
    write_schema::<GitCommitReply>("com_nexus_git__commit", "reply");
    write_schema::<GitOk>("com_nexus_git", "ok");
}

/// Audit-2026-05-01 P0-2: every emitted JSON schema for an object type
/// must declare `additionalProperties: false`. This is the gate that
/// locks in the workspace-wide `#[serde(deny_unknown_fields)]` rollout
/// from P0-1 — without this assertion a future struct could be added
/// without the attribute and silently slip past code review.
///
/// Recurses into nested object types under `definitions` / `$defs` /
/// `properties.<x>` so a single struct exposing nested object types
/// is policed in full. Non-object schemas (string/number/enum) are
/// ignored because `additionalProperties` is meaningless for them.
#[test]
fn every_object_schema_denies_additional_properties() {
    // Re-run emission so this test is independent of ordering.
    emit_pilot_ipc_schemas();

    let mut violations: Vec<String> = Vec::new();
    for entry in fs::read_dir(out_dir()).expect("read schemas/ipc") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let value: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        let label = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>")
            .to_string();
        check_strict_objects(&value, &label, "$", &mut violations);
    }

    assert!(
        violations.is_empty(),
        "schemas missing additionalProperties: false (audit-2026-05-01 P0-2):\n  {}",
        violations.join("\n  "),
    );
}

/// Recurse `value`, asserting that every object-typed schema sets
/// `additionalProperties: false`. Walks `definitions`, `$defs`,
/// `properties.*`, `items`, `anyOf`, `oneOf`, `allOf`. Tolerates
/// schemas that omit `type` (those describe a union or a $ref).
fn check_strict_objects(
    value: &serde_json::Value,
    file: &str,
    path: &str,
    violations: &mut Vec<String>,
) {
    if value.get("type").and_then(serde_json::Value::as_str) == Some("object") {
        // Accept `false` (struct), object (typed map), or `true`
        // (any-value map). Missing means a struct without
        // `deny_unknown_fields` — what P0-2 forbids.
        match value.get("additionalProperties") {
            Some(serde_json::Value::Bool(_)) => {}
            Some(serde_json::Value::Object(_)) => {
                if let Some(inner) = value.get("additionalProperties") {
                    check_strict_objects(
                        inner,
                        file,
                        &format!("{path}.additionalProperties"),
                        violations,
                    );
                }
            }
            _ => violations.push(format!("{file} :: {path}")),
        }
    }
    for key in ["definitions", "$defs", "properties"] {
        if let Some(map) = value.get(key).and_then(serde_json::Value::as_object) {
            for (sub_key, sub) in map {
                check_strict_objects(sub, file, &format!("{path}.{key}.{sub_key}"), violations);
            }
        }
    }
    if let Some(items) = value.get("items") {
        check_strict_objects(items, file, &format!("{path}.items"), violations);
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(arr) = value.get(key).and_then(serde_json::Value::as_array) {
            for (i, sub) in arr.iter().enumerate() {
                check_strict_objects(sub, file, &format!("{path}.{key}[{i}]"), violations);
            }
        }
    }
}

/// Sanity check: after emission the 5 pilot handlers each have at least
/// an `args` and a `result` file on disk.
#[test]
fn every_pilot_handler_has_args_and_result() {
    // Re-run emission so this test is independent of ordering.
    emit_pilot_ipc_schemas();

    let handlers = [
        "com_nexus_storage__search",
        "com_nexus_storage__read_file",
        "com_nexus_storage__write_file",
        "com_nexus_storage__list_dir",
        "com_nexus_ai__stream_ask",
    ];
    for h in handlers {
        for role in ["args", "result"] {
            let path = out_dir().join(format!("{h}_{role}.json"));
            assert!(
                path.exists(),
                "expected JSON Schema to exist at {} — did emit_pilot_ipc_schemas run?",
                path.display(),
            );
        }
    }
}
