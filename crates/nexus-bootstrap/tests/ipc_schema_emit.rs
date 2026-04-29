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
    AiStreamAskArgs, AiStreamAskMessage, AiStreamAskResult, AiStreamAskRole, AiStreamAskSource,
    AiStreamChatArgs, AiStreamChatMode, AiToolPolicy,
};
// FU-13 — RAG response shape (BL-038). TS bindings shipped already;
// emitting JSON Schema lets MCP / external tools consume the same
// contract over the wire.
use nexus_ai::{Citation, RagResponse};
use nexus_storage::ipc::{
    StorageListDirArgs, StorageListDirEntry, StorageListDirResult, StorageNoteAppendArgs,
    StorageNoteAppendResult, StorageReadFileArgs, StorageReadFileResult, StorageSearchArgs,
    StorageSearchHit, StorageSearchResult, StorageWriteFileArgs, StorageWriteFileResult,
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
