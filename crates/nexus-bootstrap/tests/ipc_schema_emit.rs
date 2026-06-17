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
    AiActivityListArgs, AiActivityListResult, AiGenerateDocsArgs, AiGenerateDocsReply,
    AiPredictArgs, AiPredictReply, AiProposeArgs, AiProposeReply, AiProposedToolCall,
    AiStreamAskArgs, AiStreamAskMessage, AiStreamAskResult, AiStreamAskRole, AiStreamAskSource,
    AiStreamChatArgs, AiStreamChatMode, AiToolPolicy, AiUnmappedToolCall, EnrichEntityArgs,
    EnrichEntityResult, EntityRecallArgs, EntityRecallHitRow, EntityRecallResult,
    InferEntityRelationsArgs, InferEntityRelationsResult, InferredRelationRow,
};
// FU-13 — RAG response shape (BL-038). TS bindings shipped already;
// emitting JSON Schema lets MCP / external tools consume the same
// contract over the wire.
// BL-052 — activity types now live in `nexus_types::activity`.
use nexus_ai::{Citation, RagResponse};
use nexus_storage::ipc::{
    DraftRelationRow, EntityDecayRelationsArgs, EntityDecayRelationsResult, EntityDuplicatePairRow,
    EntityFindDuplicatesArgs, EntityFindDuplicatesResult, EntityGetArgs, EntityGetResult,
    EntityMergeArgs, EntityMergeResult, EntityRecordRow, EntityRelationRow, EntityRelationsArgs,
    EntityRelationsResult, EntityRelationsResultRow, EntitySearchArgs, EntitySearchHitRow,
    EntitySearchResult, EntityUpsertArgs, EntityUpsertRelationRow, EntityUpsertResult,
    ListDraftRelationsArgs, ListDraftRelationsResult, ReadFrontmatterResult,
    StorageBacklinksToBlockArgs, StorageBaseCreateArgs, StorageBaseIndexResult,
    StorageBaseNamedArgs, StorageBasePropertyCreateArgs, StorageBasePropertyRenameArgs,
    StorageBasePropertyUpdateArgs, StorageBaseQueryArgs, StorageBaseRecordCreateArgs,
    StorageBaseRecordIdArgs, StorageBaseRecordUpdateArgs, StorageBaseViewArgs,
    StorageCanvasPatchArgs, StorageCanvasWriteArgs, StorageChunkEmbedding,
    StorageConfigContentResult, StorageConfigKindArgs, StorageEditArgs, StorageEditConflict,
    StorageEditFileResult, StorageEditResult, StorageFileExistsResult,
    StorageGraphNeighborsArgs, StorageImportConflictStrategy, StorageImportForgeArgs,
    StorageListDirArgs, StorageListDirEntry, StorageListDirResult, StorageNoteAppendArgs,
    StorageNoteAppendResult, StorageOk, StoragePathArgs, StorageQuerySymbolArgs,
    StorageQuerySymbolResult, StorageQueryTagsArgs, StorageReadFileArgs, StorageReadFileResult,
    StorageReadFrontmatterArgs, StorageRelpathArgs, StorageRenameEntryArgs, StorageSearchArgs,
    StorageSearchHit, StorageSearchResult, StorageSettingsWriteArgs, StorageSymbolRow,
    StorageToggleTaskArgs, StorageVectorInsertArgs, StorageVectorMatch, StorageVectorQueryArgs,
    StorageVectorstoreCountResult, StorageWriteFileArgs, StorageWriteFileResult,
    StorageWriteFrontmatterArgs,
};
use nexus_types::activity::{ActivityEntry, ActivityOutcome, ActivitySurface, ActivityToolCall};
// Audit-2026-05-01 P1-3 (#113): linkpreview is the first subsystem
// brought into the schema generator outside the original storage / ai
// pilot.
use nexus_linkpreview::core_plugin::FetchArgs as LinkPreviewFetchArgs;
use nexus_linkpreview::LinkPreview;
// BL-133 — multi-channel notification dispatcher.
// BL-136 — inbox IPC surface (`inbox_list` / `inbox_mark_read` /
// `inbox_dismiss` / `inbox_stats`).
use nexus_notifications::core_plugin::{
    InboxIdsArgs as NotificationsInboxIdsArgs, InboxListArgs as NotificationsInboxListArgs,
    InboxUpdatedReply as NotificationsInboxUpdatedReply, SendArgs as NotificationsSendArgs,
    SendReply as NotificationsSendReply,
};
use nexus_notifications::inbox::{
    InboxEntry as NotificationsInboxEntry, InboxStats as NotificationsInboxStats,
};
use nexus_notifications::Channel as NotificationsChannel;
// BL-134 / ADR 0028 Phase 1 — `com.nexus.ai.runtime` task scheduler.
use nexus_ai_runtime::events::AiEvent as AiRuntimeEvent;
use nexus_ai_runtime::{
    AgentRun, AgentRunSummary, AgentTaskKind, AiRuntimeControlArgs, AiRuntimeEventsArgs,
    AiRuntimeGetArgs, AiRuntimeListArgs, AiRuntimeListTriggersReply, AiRuntimeRegisterTriggerArgs,
    AiRuntimeRegisterTriggerReply, AiRuntimeSubmitArgs, AiRuntimeSubmitReply,
    AiRuntimeUnregisterTriggerArgs, AiRuntimeUnregisterTriggerReply, AiRuntimeWaitForArgs,
    AiRuntimeWaitForReply, AmbientTrigger, EventInput, EventInputMode, PoolStats, RunStatus,
    TaskPriority, TriggerFilter, TriggerId,
};
// nexus-git uses a wire-mirror module — handlers emit ad-hoc
// `serde_json::json!` and the impl types in `nexus_git::types`
// don't even derive `Serialize`.
use nexus_git::ipc::{
    GitBranch, GitCommitArgs, GitCommitReply, GitDiffHunk, GitDiffLine, GitLogArgs, GitLogEntry,
    GitOk, GitPathArgs, GitStatusReply,
};
// nexus-mcp uses a wire-mirror module (`nexus_mcp::ipc`) — the
// existing handlers construct ad-hoc JSON via `serde_json::json!`.
use nexus_mcp::ipc::{
    McpCallToolArgs, McpCallToolReply, McpConnectReply, McpDisconnectMissReply, McpPromptEntry,
    McpRegisterServerArgs, McpRegisterServerReply, McpResourceEntry, McpServerArgs, McpServerEntry,
    McpToolEntry, McpUnregisterServerArgs, McpUnregisterServerReply,
};
// nexus-lsp uses a wire-mirror module — the handlers emit ad-hoc
// `serde_json::json!` and accept `Value` in (BL-076).
use nexus_lsp::ipc::{
    LspChangeFileArgs, LspCodeActionsArgs, LspExecuteCommandArgs, LspOk, LspOpenFileArgs,
    LspOpenFileReply, LspPathArgs, LspPositionArgs, LspReferencesArgs, LspRegisterServerArgs,
    LspRegisterServerReply, LspRenameArgs, LspServerEntry, LspUnregisterServerArgs,
    LspUnregisterServerReply,
};
// nexus-dap (BL-081) — wire-mirror types; handlers emit ad-hoc
// `serde_json::json!` like nexus-lsp.
use nexus_dap::ipc::{
    DapAdapterArgs, DapAdapterEntry, DapAttachArgs, DapEvaluateArgs, DapFunctionBreakpoint,
    DapLaunchArgs, DapOk, DapRegisterAdapterArgs, DapRegisterAdapterReply, DapScopesArgs,
    DapSetBreakpointsArgs, DapSetExceptionBreakpointsArgs, DapSetFunctionBreakpointsArgs,
    DapSourceBreakpoint, DapStackTraceArgs, DapThreadArgs, DapUnregisterAdapterArgs,
    DapUnregisterAdapterReply, DapVariablesArgs,
};
// nexus-acp (BL-144 / Hermes Feature 7) — wire-mirror types; handlers
// emit ad-hoc `serde_json::json!` like nexus-lsp / nexus-dap.
use nexus_acp::ipc::{
    AcpAgentArgs, AcpAgentEntry, AcpDecisionArgs, AcpProposeArgs, AcpRegisterServerArgs,
    AcpRegisterServerReply, AcpUnregisterServerArgs, AcpUnregisterServerReply,
};
use nexus_agent::core_plugin::{GoalArgs, PlanIdArgs};
use nexus_agent::transcript_search::{SearchArgs as TranscriptSearchArgs, TranscriptHit};
use nexus_agent::{Plan, Step, ToolCall};
use nexus_comments::core_plugin::{
    AddReplyArgs, CreateThreadArgs, DeleteCommentArgs, DeleteThreadArgs, EditCommentArgs,
    FilePathArg, SetResolvedArgs,
};
use nexus_comments::{Comment, Thread};
use nexus_theme::api::{AppliedTheme, SnippetMetadata, ThemeConfig};
use nexus_theme::core_plugin::{
    Ack as ThemeAck, ApplyConfigArgs, ApplyThemeArgs, ComputeVariablesArgs, ReorderSnippetsArgs,
    SetModeArgs, SetPluginOverridesArgs, ToggleSnippetArgs,
};
use nexus_theme::snippet::{SnippetMode, SnippetScope};
use nexus_theme::ThemeMode;
// nexus-skills exposes only its Args types — the return Skill/SkillMeta
// uses `#[serde(flatten)] extra: BTreeMap<String, serde_yml::Value>`
// for forward-compat YAML, which is fundamentally incompatible with
// `deny_unknown_fields`. Shell-side consumers treat Skill as opaque.
use nexus_skills::core_plugin::{
    ComposeSkillArgs, GetSkillArgs, InvokeSkillArgs, ListByContextArgs, RenderSkillArgs,
    TriggeredByArgs,
};
// nexus-workflow exposes only its Args types — Workflow / Trigger /
// Step / DigestConfig use `#[serde(flatten)] extra: BTreeMap<String,
// toml::Value>` for forward-compat, which is incompatible with
// `deny_unknown_fields` (the P0-2 gate's invariant).
use nexus_terminal::{
    CreateSessionArgs, CreateSessionResponse, OutputStreamPayload, PumpArgs, PumpResponse,
    ReadOutputArgs, ReadRawSinceArgs, ReadRawSinceResponse, ReplEvalArgs, ReplInfo, ReplStartArgs,
    ReplStartResponse, ResizeArgs, SearchOutputArgs, SendInputArgs, SendRawInputArgs,
    SessionIdArgs, WaitForPatternArgs, WaitForPatternResponse,
};
use nexus_workflow::core_plugin::{
    GetTemplateArgs, GetWorkflowArgs, InitTemplateArgs, NextFireArgs, RunHistoryArgs,
    RunWorkflowArgs, ValidateWorkflowArgs,
};
// nexus-database — only the 4 args/responses that don't reference
// `nexus_types::bases::BaseRecord` are wired in. BaseRecord uses
// `#[serde(flatten)]` for forward-compat record fields, which is
// incompatible with the P0-2 `deny_unknown_fields` gate.
use nexus_database::core_plugin::{
    CsvExportResponse, CsvImportArgs, FormulaEvalArgs, FormulaEvalResponse,
};
// nexus-templates: the four args types for the page-template subsystem.
// Note `GetTemplateArgs` collides with the workflow-templates type, so
// we alias on import.
use nexus_templates::core_plugin::{ApplyTemplateArgs, GetPageTemplateArgs, RenderTemplateArgs};
// nexus-formats: Notion zip-import / format-export args.
use nexus_formats::core_plugin::{ExportNotionArgs, ImportNotionArgs};
// nexus-audio (BL-117): STT + TTS IPC types.
use nexus_audio::ipc::{
    AudioStatusResult, AudioSynthesizeArgs, AudioSynthesizeResult, AudioTranscribeArgs,
    AudioTranscribeResult,
};
// nexus-editor (P1-3 #113) — wire-mirror types; handlers parse args
// from `Value` and emit responses with `serde_json::json!`.
use nexus_editor::ipc::{
    EditorApplyTransactionArgs, EditorExcerptRequest, EditorOk, EditorOpenExcerptsArgs,
    EditorPathArgs, EditorResolveBlockLinkArgs, EditorStampBlockArgs, EditorStampBlockReply,
    EditorSyncContentArgs,
};

/// Relative path under `crates/nexus-bootstrap/schemas/ipc/`. Emits
/// `<plugin>_<command>_<suffix>.json` so sibling types for the same
/// handler (args/result/hit/…) land next to each other alphabetically.
fn write_schema<T: JsonSchema>(handler_slug: &str, role: &str) {
    let schema = schema_for!(T);
    let pretty = serde_json::to_string_pretty(&schema).expect("schema serializes to JSON") + "\n";
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
    emit_all_schemas();
}

/// Idempotent under parallel-test contention: the strict-objects and
/// per-handler tests both call this, but cargo runs `#[test]` fns in
/// parallel so without a guard each call would re-truncate the same
/// JSON files concurrently and the strict-objects pass would observe
/// a half-written EOF. The `OnceLock` ensures we emit exactly once
/// per test-binary invocation.
fn emit_all_schemas() {
    use std::sync::OnceLock;
    static EMITTED: OnceLock<()> = OnceLock::new();
    EMITTED.get_or_init(emit_all_schemas_impl);
}

#[allow(clippy::too_many_lines)] // 100+ schema emits — splitting hurts readability
fn emit_all_schemas_impl() {
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

    // ── com.nexus.storage::edit (Phase 5.1 / RFC 0005) ───────────────────
    write_schema::<StorageEditArgs>("com_nexus_storage__edit", "args");
    write_schema::<StorageEditFileResult>("com_nexus_storage__edit", "file_result");
    write_schema::<StorageEditConflict>("com_nexus_storage__edit", "conflict");
    write_schema::<StorageEditResult>("com_nexus_storage__edit", "result");

    // ── com.nexus.storage::note_append (BL-043) ──────────────────────────
    write_schema::<StorageNoteAppendArgs>("com_nexus_storage__note_append", "args");
    write_schema::<StorageNoteAppendResult>("com_nexus_storage__note_append", "result");

    // ── com.nexus.storage::list_dir ──────────────────────────────────────
    write_schema::<StorageListDirArgs>("com_nexus_storage__list_dir", "args");
    write_schema::<StorageListDirEntry>("com_nexus_storage__list_dir", "entry");
    write_schema::<StorageListDirResult>("com_nexus_storage__list_dir", "result");

    // ── com.nexus.storage::read_frontmatter (BL-053 Phase 4) ─────────────
    write_schema::<StorageReadFrontmatterArgs>("com_nexus_storage__read_frontmatter", "args");
    write_schema::<ReadFrontmatterResult>("com_nexus_storage__read_frontmatter", "result");

    // ── com.nexus.storage::write_frontmatter (#190) ──────────────────────
    write_schema::<StorageWriteFrontmatterArgs>("com_nexus_storage__write_frontmatter", "args");
    write_schema::<StorageOk>("com_nexus_storage__write_frontmatter", "result");

    // ── com.nexus.storage::delete_file / file_exists / write_vault_file (#190) ──
    // All three share `StoragePathArgs` for the request shape; only
    // `file_exists` carries a non-`StorageOk` reply.
    write_schema::<StoragePathArgs>("com_nexus_storage__path_args", "shared");
    write_schema::<StorageFileExistsResult>("com_nexus_storage__file_exists", "result");

    // ── com.nexus.storage::vector_* (#190) ───────────────────────────────
    write_schema::<StorageChunkEmbedding>("com_nexus_storage__vector", "chunk");
    write_schema::<StorageVectorInsertArgs>("com_nexus_storage__vector_insert", "args");
    write_schema::<StorageVectorQueryArgs>("com_nexus_storage__vector_query", "args");
    write_schema::<StorageVectorMatch>("com_nexus_storage__vector_query", "match");
    write_schema::<StorageVectorstoreCountResult>("com_nexus_storage__vectorstore_count", "result");

    // ── com.nexus.storage::base_record_* delete/restore + base_*_delete + rename (#190) ──
    write_schema::<StorageBaseRecordIdArgs>("com_nexus_storage__base_record_id", "args");
    write_schema::<StorageBaseNamedArgs>("com_nexus_storage__base_named", "args");
    write_schema::<StorageBasePropertyRenameArgs>(
        "com_nexus_storage__base_property_rename",
        "args",
    );

    // ── com.nexus.storage::base_* complex args (#190) ────────────────────
    // Outer envelopes are strict; inner domain types (BaseRecord,
    // BaseView, BaseSchema, property definitions) pass through as
    // `serde_json::Value` because the impl types aren't yet
    // `JsonSchema`-derive friendly.
    write_schema::<StorageBaseRecordCreateArgs>("com_nexus_storage__base_record_create", "args");
    write_schema::<StorageBaseRecordUpdateArgs>("com_nexus_storage__base_record_update", "args");
    write_schema::<StorageBasePropertyCreateArgs>(
        "com_nexus_storage__base_property_create",
        "args",
    );
    write_schema::<StorageBasePropertyUpdateArgs>(
        "com_nexus_storage__base_property_update",
        "args",
    );
    write_schema::<StorageBaseViewArgs>("com_nexus_storage__base_view", "args");
    write_schema::<StorageBaseCreateArgs>("com_nexus_storage__base_create", "args");
    write_schema::<StorageBaseQueryArgs>("com_nexus_storage__base_query", "args");
    write_schema::<StorageBaseIndexResult>("com_nexus_storage__base_index", "result");

    // ── com.nexus.storage::import_forge (#190) ───────────────────────────
    write_schema::<StorageImportForgeArgs>("com_nexus_storage__import_forge", "args");
    write_schema::<StorageImportConflictStrategy>(
        "com_nexus_storage__import_forge",
        "conflict_strategy",
    );

    // ── com.nexus.storage::tree handlers (#190) ──────────────────────────
    write_schema::<StorageRelpathArgs>("com_nexus_storage__relpath_args", "shared");
    write_schema::<StorageRenameEntryArgs>("com_nexus_storage__rename_entry", "args");

    // ── com.nexus.storage::query_tags / config_* / settings_write (#190) ─
    write_schema::<StorageQueryTagsArgs>("com_nexus_storage__query_tags", "args");
    write_schema::<StorageConfigKindArgs>("com_nexus_storage__config_kind", "args");
    write_schema::<StorageConfigContentResult>("com_nexus_storage__config_read", "result");
    write_schema::<StorageSettingsWriteArgs>("com_nexus_storage__settings_write", "args");

    // ── com.nexus.storage::canvas / tasks / graph (#190) ─────────────────
    write_schema::<StorageCanvasWriteArgs>("com_nexus_storage__canvas_write", "args");
    write_schema::<StorageCanvasPatchArgs>("com_nexus_storage__canvas_patch", "args");
    write_schema::<StorageToggleTaskArgs>("com_nexus_storage__toggle_task", "args");
    write_schema::<StorageBacklinksToBlockArgs>("com_nexus_storage__backlinks_to_block", "args");
    write_schema::<StorageGraphNeighborsArgs>("com_nexus_storage__graph_neighbors", "args");

    // ── com.nexus.storage::query_symbol (BL-114) ─────────────────────────
    write_schema::<StorageQuerySymbolArgs>("com_nexus_storage__query_symbol", "args");
    write_schema::<StorageSymbolRow>("com_nexus_storage__query_symbol", "row");
    write_schema::<StorageQuerySymbolResult>("com_nexus_storage__query_symbol", "result");

    // ── com.nexus.storage::entity_search / entity_get / entity_relations (BL-128) ──
    write_schema::<EntitySearchArgs>("com_nexus_storage__entity_search", "args");
    write_schema::<EntitySearchHitRow>("com_nexus_storage__entity_search", "hit");
    write_schema::<EntitySearchResult>("com_nexus_storage__entity_search", "result");
    write_schema::<EntityGetArgs>("com_nexus_storage__entity_get", "args");
    write_schema::<EntityRecordRow>("com_nexus_storage__entity_get", "row");
    write_schema::<EntityRelationRow>("com_nexus_storage__entity_get", "relation");
    write_schema::<EntityGetResult>("com_nexus_storage__entity_get", "result");
    write_schema::<EntityRelationsArgs>("com_nexus_storage__entity_relations", "args");
    write_schema::<EntityRelationsResultRow>("com_nexus_storage__entity_relations", "row");
    write_schema::<EntityRelationsResult>("com_nexus_storage__entity_relations", "result");
    write_schema::<EntityUpsertRelationRow>("com_nexus_storage__entity_upsert", "relation");
    write_schema::<EntityUpsertArgs>("com_nexus_storage__entity_upsert", "args");
    write_schema::<EntityUpsertResult>("com_nexus_storage__entity_upsert", "result");
    write_schema::<EntityFindDuplicatesArgs>("com_nexus_storage__entity_find_duplicates", "args");
    write_schema::<EntityDuplicatePairRow>("com_nexus_storage__entity_find_duplicates", "pair");
    write_schema::<EntityFindDuplicatesResult>(
        "com_nexus_storage__entity_find_duplicates",
        "result",
    );
    write_schema::<EntityDecayRelationsArgs>("com_nexus_storage__entity_decay_relations", "args");
    write_schema::<EntityDecayRelationsResult>(
        "com_nexus_storage__entity_decay_relations",
        "result",
    );
    write_schema::<EntityMergeArgs>("com_nexus_storage__entity_merge", "args");
    write_schema::<EntityMergeResult>("com_nexus_storage__entity_merge", "result");
    write_schema::<ListDraftRelationsArgs>("com_nexus_storage__list_draft_relations", "args");
    write_schema::<DraftRelationRow>("com_nexus_storage__list_draft_relations", "row");
    write_schema::<ListDraftRelationsResult>("com_nexus_storage__list_draft_relations", "result");
    write_schema::<EnrichEntityArgs>("com_nexus_ai__enrich_entity", "args");
    write_schema::<EnrichEntityResult>("com_nexus_ai__enrich_entity", "result");
    write_schema::<InferEntityRelationsArgs>("com_nexus_ai__infer_entity_relations", "args");
    write_schema::<InferredRelationRow>("com_nexus_ai__infer_entity_relations", "row");
    write_schema::<InferEntityRelationsResult>("com_nexus_ai__infer_entity_relations", "result");

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

    // ── com.nexus.ai::propose_tool_calls (G7 / ADR 0023) ─────────────────
    // Single-turn provider call that returns mapped tool-use blocks
    // without executing them; consumed by the agent migration.
    write_schema::<AiProposeArgs>("com_nexus_ai__propose_tool_calls", "args");
    write_schema::<AiProposeReply>("com_nexus_ai__propose_tool_calls", "reply");
    write_schema::<AiProposedToolCall>("com_nexus_ai__propose_tool_calls", "tool_call");
    write_schema::<AiUnmappedToolCall>("com_nexus_ai__propose_tool_calls", "unmapped");

    // ── com.nexus.ai::ask (BL-038 RAG response) ──────────────────────────
    // The MCP surface re-uses this shape to expose RAG answers + their
    // citation list; emitting the JSON Schema keeps MCP-side decoders
    // honest as `Citation` evolves.
    write_schema::<Citation>("com_nexus_ai__ask", "citation");
    write_schema::<RagResponse>("com_nexus_ai__ask", "result");

    // ── com.nexus.ai::generate_docs (BL-116) ─────────────────────────────
    write_schema::<AiGenerateDocsArgs>("com_nexus_ai__generate_docs", "args");
    write_schema::<AiGenerateDocsReply>("com_nexus_ai__generate_docs", "reply");

    // ── com.nexus.ai::predict (BL-139) ──────────────────────────────────
    // Per-keystroke FIM edit prediction; consumed by the CM6
    // editPrediction extension in the shell.
    write_schema::<AiPredictArgs>("com_nexus_ai__predict", "args");
    write_schema::<AiPredictReply>("com_nexus_ai__predict", "reply");

    // ── com.nexus.ai::entity_recall (BL-128 close) ──────────────────────
    write_schema::<EntityRecallArgs>("com_nexus_ai__entity_recall", "args");
    write_schema::<EntityRecallHitRow>("com_nexus_ai__entity_recall", "hit");
    write_schema::<EntityRecallResult>("com_nexus_ai__entity_recall", "result");

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

    // ── com.nexus.notifications::send (BL-133) ───────────────────────────
    // Multi-channel notification dispatcher — desktop / discord /
    // telegram / email (BL-133 follow-up). Args + reply are wire-tagged
    // through a single `Channel` enum; the schema makes the wire form
    // explicit for shell-side and 3rd-party consumers.
    write_schema::<NotificationsSendArgs>("com_nexus_notifications__send", "args");
    write_schema::<NotificationsSendReply>("com_nexus_notifications__send", "reply");
    write_schema::<NotificationsChannel>("com_nexus_notifications", "channel");

    // ── com.nexus.notifications inbox surface (BL-136) ────────────
    write_schema::<NotificationsInboxListArgs>("com_nexus_notifications__inbox_list", "args");
    write_schema::<NotificationsInboxIdsArgs>("com_nexus_notifications__inbox_ids", "args");
    write_schema::<NotificationsInboxUpdatedReply>(
        "com_nexus_notifications__inbox_updated",
        "reply",
    );
    write_schema::<NotificationsInboxEntry>("com_nexus_notifications__inbox", "entry");
    write_schema::<NotificationsInboxStats>("com_nexus_notifications__inbox", "stats");

    // ── com.nexus.ai.runtime (BL-134 Phase 1, ADR 0028) ─────────────────
    // Task scheduler + typed AiEvent envelope. Phase 1 wires submit /
    // get / list / events / pool_stats; the cancel/pause/resume IDs are
    // reserved for Phase 5 but their args type already ships so the
    // wire shape is locked.
    write_schema::<AiRuntimeSubmitArgs>("com_nexus_ai_runtime__submit", "args");
    write_schema::<AiRuntimeSubmitReply>("com_nexus_ai_runtime__submit", "reply");
    write_schema::<AiRuntimeGetArgs>("com_nexus_ai_runtime__get", "args");
    write_schema::<AiRuntimeListArgs>("com_nexus_ai_runtime__list", "args");
    write_schema::<AiRuntimeEventsArgs>("com_nexus_ai_runtime__events", "args");
    write_schema::<AiRuntimeControlArgs>("com_nexus_ai_runtime__control", "args");
    // Phase 2 — sync wait-until-terminal primitive.
    write_schema::<AiRuntimeWaitForArgs>("com_nexus_ai_runtime__wait_for", "args");
    write_schema::<AiRuntimeWaitForReply>("com_nexus_ai_runtime__wait_for", "reply");
    write_schema::<AgentTaskKind>("com_nexus_ai_runtime", "task_kind");
    write_schema::<AgentRun>("com_nexus_ai_runtime__get", "result");
    write_schema::<AgentRunSummary>("com_nexus_ai_runtime__list", "summary");
    write_schema::<AiRuntimeEvent>("com_nexus_ai_runtime", "event");
    write_schema::<PoolStats>("com_nexus_ai_runtime__pool_stats", "result");
    write_schema::<RunStatus>("com_nexus_ai_runtime", "run_status");
    write_schema::<TaskPriority>("com_nexus_ai_runtime", "task_priority");
    // Move 7 — AmbientTrigger / trigger watcher IPC surface.
    write_schema::<AiRuntimeRegisterTriggerArgs>("com_nexus_ai_runtime__register_trigger", "args");
    write_schema::<AiRuntimeRegisterTriggerReply>(
        "com_nexus_ai_runtime__register_trigger",
        "reply",
    );
    write_schema::<AiRuntimeUnregisterTriggerArgs>(
        "com_nexus_ai_runtime__unregister_trigger",
        "args",
    );
    write_schema::<AiRuntimeUnregisterTriggerReply>(
        "com_nexus_ai_runtime__unregister_trigger",
        "reply",
    );
    write_schema::<AiRuntimeListTriggersReply>("com_nexus_ai_runtime__list_triggers", "reply");
    write_schema::<AmbientTrigger>("com_nexus_ai_runtime", "ambient_trigger");
    write_schema::<TriggerFilter>("com_nexus_ai_runtime", "trigger_filter");
    write_schema::<EventInput>("com_nexus_ai_runtime", "event_input");
    write_schema::<EventInputMode>("com_nexus_ai_runtime", "event_input_mode");
    write_schema::<TriggerId>("com_nexus_ai_runtime", "trigger_id");

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

    // ── com.nexus.mcp.host (P1-3 #113) ───────────────────────────────────
    // Wire-mirror types — the impl emits ad-hoc `serde_json::json!`.
    write_schema::<McpServerArgs>("com_nexus_mcp_host", "server_args");
    write_schema::<McpCallToolArgs>("com_nexus_mcp_host__call_tool", "args");
    write_schema::<McpServerEntry>("com_nexus_mcp_host__list_servers", "entry");
    write_schema::<McpToolEntry>("com_nexus_mcp_host__list_tools", "entry");
    write_schema::<McpResourceEntry>("com_nexus_mcp_host__list_resources", "entry");
    write_schema::<McpPromptEntry>("com_nexus_mcp_host__list_prompts", "entry");
    write_schema::<McpConnectReply>("com_nexus_mcp_host__connect", "reply");
    write_schema::<McpDisconnectMissReply>("com_nexus_mcp_host__disconnect", "miss_reply");
    write_schema::<McpCallToolReply>("com_nexus_mcp_host__call_tool", "reply");
    // BL-113 Phase 3b — plugin contribution registration verbs.
    write_schema::<McpRegisterServerArgs>("com_nexus_mcp_host__register_server", "args");
    write_schema::<McpRegisterServerReply>("com_nexus_mcp_host__register_server", "reply");
    write_schema::<McpUnregisterServerArgs>("com_nexus_mcp_host__unregister_server", "args");
    write_schema::<McpUnregisterServerReply>("com_nexus_mcp_host__unregister_server", "reply");

    // ── com.nexus.lsp (BL-076) ───────────────────────────────────────────
    // Wire-mirror types — the impl emits ad-hoc `serde_json::json!`.
    write_schema::<LspServerEntry>("com_nexus_lsp__list_servers", "entry");
    write_schema::<LspOpenFileArgs>("com_nexus_lsp__open_file", "args");
    write_schema::<LspOpenFileReply>("com_nexus_lsp__open_file", "reply");
    write_schema::<LspPathArgs>("com_nexus_lsp", "path_args");
    write_schema::<LspChangeFileArgs>("com_nexus_lsp__change_file", "args");
    write_schema::<LspPositionArgs>("com_nexus_lsp", "position_args");
    write_schema::<LspReferencesArgs>("com_nexus_lsp__references", "args");
    write_schema::<LspRenameArgs>("com_nexus_lsp__rename", "args");
    write_schema::<LspCodeActionsArgs>("com_nexus_lsp__code_actions", "args");
    write_schema::<LspExecuteCommandArgs>("com_nexus_lsp__execute_command", "args");
    write_schema::<LspOk>("com_nexus_lsp", "ok");
    // BL-113 Phase 2b — plugin contribution registration verbs.
    write_schema::<LspRegisterServerArgs>("com_nexus_lsp__register_server", "args");
    write_schema::<LspRegisterServerReply>("com_nexus_lsp__register_server", "reply");
    write_schema::<LspUnregisterServerArgs>("com_nexus_lsp__unregister_server", "args");
    write_schema::<LspUnregisterServerReply>("com_nexus_lsp__unregister_server", "reply");

    // ── com.nexus.dap (BL-081) ───────────────────────────────────────────
    // Wire-mirror types — the impl emits ad-hoc `serde_json::json!`.
    write_schema::<DapAdapterEntry>("com_nexus_dap__list_adapters", "entry");
    write_schema::<DapLaunchArgs>("com_nexus_dap__launch", "args");
    write_schema::<DapAttachArgs>("com_nexus_dap__attach", "args");
    write_schema::<DapAdapterArgs>("com_nexus_dap", "adapter_args");
    write_schema::<DapSourceBreakpoint>("com_nexus_dap", "source_breakpoint");
    write_schema::<DapSetBreakpointsArgs>("com_nexus_dap__set_breakpoints", "args");
    write_schema::<DapFunctionBreakpoint>("com_nexus_dap", "function_breakpoint");
    write_schema::<DapSetFunctionBreakpointsArgs>(
        "com_nexus_dap__set_function_breakpoints",
        "args",
    );
    write_schema::<DapSetExceptionBreakpointsArgs>(
        "com_nexus_dap__set_exception_breakpoints",
        "args",
    );
    write_schema::<DapThreadArgs>("com_nexus_dap", "thread_args");
    write_schema::<DapStackTraceArgs>("com_nexus_dap__stack_trace", "args");
    write_schema::<DapScopesArgs>("com_nexus_dap__scopes", "args");
    write_schema::<DapVariablesArgs>("com_nexus_dap__variables", "args");
    write_schema::<DapEvaluateArgs>("com_nexus_dap__evaluate", "args");
    write_schema::<DapOk>("com_nexus_dap", "ok");
    // BL-113 Phase 1b — plugin contribution registration verbs.
    write_schema::<DapRegisterAdapterArgs>("com_nexus_dap__register_adapter", "args");
    write_schema::<DapRegisterAdapterReply>("com_nexus_dap__register_adapter", "reply");
    write_schema::<DapUnregisterAdapterArgs>("com_nexus_dap__unregister_adapter", "args");
    write_schema::<DapUnregisterAdapterReply>("com_nexus_dap__unregister_adapter", "reply");

    // ── com.nexus.acp (BL-144) ───────────────────────────────────────────
    write_schema::<AcpAgentArgs>("com_nexus_acp", "agent_args");
    write_schema::<AcpProposeArgs>("com_nexus_acp__propose", "args");
    write_schema::<AcpDecisionArgs>("com_nexus_acp", "decision_args");
    write_schema::<AcpAgentEntry>("com_nexus_acp__list_agents", "entry");
    // BL-113 Phase 4 — plugin contribution registration verbs.
    write_schema::<AcpRegisterServerArgs>("com_nexus_acp__register_server", "args");
    write_schema::<AcpRegisterServerReply>("com_nexus_acp__register_server", "reply");
    write_schema::<AcpUnregisterServerArgs>("com_nexus_acp__unregister_server", "args");
    write_schema::<AcpUnregisterServerReply>("com_nexus_acp__unregister_server", "reply");

    // ── com.nexus.agent (P1-3 #113) ──────────────────────────────────────
    write_schema::<GoalArgs>("com_nexus_agent__plan", "args");
    write_schema::<PlanIdArgs>("com_nexus_agent", "plan_id_args");
    // Shared types referenced from the args/responses above.
    write_schema::<Plan>("com_nexus_agent", "plan");
    write_schema::<Step>("com_nexus_agent", "step");
    write_schema::<ToolCall>("com_nexus_agent", "tool_call");
    // BL-121 — transcript-search wire types.
    write_schema::<TranscriptSearchArgs>("com_nexus_agent__search_transcripts", "args");
    write_schema::<TranscriptHit>("com_nexus_agent__search_transcripts", "hit");

    // ── com.nexus.comments (P1-3 #113) ───────────────────────────────────
    write_schema::<FilePathArg>("com_nexus_comments__list", "args");
    write_schema::<CreateThreadArgs>("com_nexus_comments__create_thread", "args");
    write_schema::<AddReplyArgs>("com_nexus_comments__add_reply", "args");
    write_schema::<SetResolvedArgs>("com_nexus_comments__set_resolved", "args");
    write_schema::<DeleteThreadArgs>("com_nexus_comments__delete_thread", "args");
    write_schema::<DeleteCommentArgs>("com_nexus_comments__delete_comment", "args");
    write_schema::<EditCommentArgs>("com_nexus_comments__edit_comment", "args");
    write_schema::<Comment>("com_nexus_comments", "comment");
    write_schema::<Thread>("com_nexus_comments", "thread");

    // ── com.nexus.theme (P1-3 #113) ──────────────────────────────────────
    // The shell's appearance pane drives every theme mutation through
    // these handlers. Every command's args + the four return shapes
    // (AppliedTheme / ThemeConfig / SnippetMetadata / Ack) are emitted
    // so the shell-side store can consume the contract directly.
    write_schema::<ApplyThemeArgs>("com_nexus_theme__apply_theme", "args");
    write_schema::<AppliedTheme>("com_nexus_theme__apply_theme", "result");
    write_schema::<ComputeVariablesArgs>("com_nexus_theme__compute_variables", "args");
    write_schema::<ToggleSnippetArgs>("com_nexus_theme__toggle_snippet", "args");
    write_schema::<ReorderSnippetsArgs>("com_nexus_theme__reorder_snippets", "args");
    write_schema::<SetModeArgs>("com_nexus_theme__set_mode", "args");
    write_schema::<ApplyConfigArgs>("com_nexus_theme__apply_config", "args");
    write_schema::<SetPluginOverridesArgs>("com_nexus_theme__set_plugin_overrides", "args");
    write_schema::<ThemeAck>("com_nexus_theme", "ack");
    // Shared types referenced from the args/results above. Schemars
    // inlines them under `$defs` of each consuming schema, but emitting
    // them as standalone files keeps the per-type contract addressable
    // (e.g. for documentation links and per-type version pinning).
    write_schema::<ThemeMode>("com_nexus_theme", "mode");
    write_schema::<ThemeConfig>("com_nexus_theme", "config");
    write_schema::<SnippetMetadata>("com_nexus_theme", "snippet_metadata");
    write_schema::<SnippetMode>("com_nexus_theme", "snippet_mode");
    write_schema::<SnippetScope>("com_nexus_theme", "snippet_scope");

    // ── com.nexus.skills (P1-3 #113) ─────────────────────────────────────
    // Args only — see import comment above for why Skill returns are
    // out of scope for this iteration.
    write_schema::<GetSkillArgs>("com_nexus_skills__get", "args");
    write_schema::<ListByContextArgs>("com_nexus_skills__list_by_context", "args");
    write_schema::<TriggeredByArgs>("com_nexus_skills__triggered_by", "args");
    write_schema::<RenderSkillArgs>("com_nexus_skills__render", "args");
    write_schema::<ComposeSkillArgs>("com_nexus_skills__compose", "args");
    write_schema::<InvokeSkillArgs>("com_nexus_skills__invoke", "args");

    // ── com.nexus.workflow (P1-3 #113) ───────────────────────────────────
    // Args only — see import comment for why Workflow/Trigger/Step
    // returns are out of scope for this iteration.
    write_schema::<RunWorkflowArgs>("com_nexus_workflow__run", "args");
    write_schema::<GetWorkflowArgs>("com_nexus_workflow__get", "args");
    write_schema::<GetTemplateArgs>("com_nexus_workflow__templates_get", "args");
    write_schema::<InitTemplateArgs>("com_nexus_workflow__templates_init", "args");
    write_schema::<ValidateWorkflowArgs>("com_nexus_workflow__validate", "args");
    write_schema::<RunHistoryArgs>("com_nexus_workflow__run_history", "args");
    write_schema::<NextFireArgs>("com_nexus_workflow__next_fire", "args");

    // ── com.nexus.terminal (P1-3 #113) ───────────────────────────────────
    write_schema::<CreateSessionArgs>("com_nexus_terminal__create_session", "args");
    write_schema::<CreateSessionResponse>("com_nexus_terminal__create_session", "response");
    write_schema::<SessionIdArgs>("com_nexus_terminal", "session_id_args");
    write_schema::<SendInputArgs>("com_nexus_terminal__send_input", "args");
    write_schema::<SendRawInputArgs>("com_nexus_terminal__send_raw_input", "args");
    write_schema::<PumpArgs>("com_nexus_terminal__pump", "args");
    write_schema::<PumpResponse>("com_nexus_terminal__pump", "response");
    write_schema::<ReadOutputArgs>("com_nexus_terminal__read_output", "args");
    write_schema::<ReadRawSinceArgs>("com_nexus_terminal__read_raw_since", "args");
    write_schema::<ReadRawSinceResponse>("com_nexus_terminal__read_raw_since", "response");
    write_schema::<ResizeArgs>("com_nexus_terminal__resize", "args");
    write_schema::<OutputStreamPayload>("com_nexus_terminal", "output_stream_payload");
    write_schema::<SearchOutputArgs>("com_nexus_terminal__search_output", "args");
    write_schema::<WaitForPatternArgs>("com_nexus_terminal__wait_for_pattern", "args");
    write_schema::<WaitForPatternResponse>("com_nexus_terminal__wait_for_pattern", "response");
    // BL-142 — REPL surface.
    write_schema::<ReplStartArgs>("com_nexus_terminal__repl_start", "args");
    write_schema::<ReplStartResponse>("com_nexus_terminal__repl_start", "response");
    write_schema::<ReplEvalArgs>("com_nexus_terminal__repl_eval", "args");
    write_schema::<ReplInfo>("com_nexus_terminal__repl_list", "info");

    // ── com.nexus.database (P1-3 #113) ───────────────────────────────────
    // Only the 4 types that don't reference BaseRecord (which uses
    // flatten and so can't have deny_unknown_fields).
    write_schema::<CsvImportArgs>("com_nexus_database__csv_import", "args");
    write_schema::<CsvExportResponse>("com_nexus_database__csv_export", "response");
    write_schema::<FormulaEvalArgs>("com_nexus_database__formula_eval", "args");
    write_schema::<FormulaEvalResponse>("com_nexus_database__formula_eval", "response");

    // ── com.nexus.templates ──────────────────────────────────────────────
    write_schema::<GetPageTemplateArgs>("com_nexus_templates__get", "args");
    write_schema::<RenderTemplateArgs>("com_nexus_templates__render", "args");
    write_schema::<ApplyTemplateArgs>("com_nexus_templates__apply", "args");

    // ── com.nexus.formats ────────────────────────────────────────────────
    write_schema::<ImportNotionArgs>("com_nexus_formats__import_notion", "args");
    write_schema::<ExportNotionArgs>("com_nexus_formats__export_notion", "args");

    // ── com.nexus.audio (BL-117) ─────────────────────────────────────────
    write_schema::<AudioTranscribeArgs>("com_nexus_audio__transcribe", "args");
    write_schema::<AudioTranscribeResult>("com_nexus_audio__transcribe", "result");
    write_schema::<AudioSynthesizeArgs>("com_nexus_audio__synthesize", "args");
    write_schema::<AudioSynthesizeResult>("com_nexus_audio__synthesize", "result");
    write_schema::<AudioStatusResult>("com_nexus_audio__status", "result");

    // ── com.nexus.editor (P1-3 #113) ─────────────────────────────────────
    // Wire-mirror Args + the `{}` ack reply. The structural reply
    // (`EditorSnapshot` / `ApplyTransactionResponse`) transitively
    // pulls in the block-tree domain model (`Block`, `BlockTree`,
    // `BlockProperties`, …), which uses `#[serde(flatten)]` for
    // forward-compat fields — incompatible with the P0-2
    // `deny_unknown_fields` gate. Same scope choice as `nexus-skills`
    // and `nexus-workflow`: args wired in, structural returns opaque.
    write_schema::<EditorPathArgs>("com_nexus_editor", "path_args");
    write_schema::<EditorSyncContentArgs>("com_nexus_editor__sync_content", "args");
    write_schema::<EditorStampBlockArgs>("com_nexus_editor__stamp_block", "args");
    write_schema::<EditorStampBlockReply>("com_nexus_editor__stamp_block", "reply");
    write_schema::<EditorApplyTransactionArgs>("com_nexus_editor__apply_transaction", "args");
    write_schema::<EditorResolveBlockLinkArgs>("com_nexus_editor__resolve_block_link", "args");
    write_schema::<EditorOpenExcerptsArgs>("com_nexus_editor__open_excerpts", "args");
    write_schema::<EditorExcerptRequest>("com_nexus_editor__open_excerpts", "item");
    write_schema::<EditorOk>("com_nexus_editor", "ok");
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
    emit_all_schemas();

    let mut violations: Vec<String> = Vec::new();
    for entry in fs::read_dir(out_dir()).expect("read schemas/ipc") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let value: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
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
    emit_all_schemas();

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
