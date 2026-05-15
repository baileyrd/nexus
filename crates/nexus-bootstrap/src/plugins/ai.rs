//! AI plugin registration.

use std::sync::Arc;

use anyhow::Result;
use nexus_ai::AiCorePlugin;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.ai",
                "AI",
                LifecycleFlags {
                    on_init: true,
                    // BL-041 — gracefully tear down the background
                    // indexing daemon on shutdown. (`on_start` stays
                    // false; the daemon is spawned from
                    // `wire_context` because that's the first hook
                    // with the kernel context in hand.)
                    on_stop: true,
                    ..LifecycleFlags::NONE
                },
                &with_v1_aliases(&[
                    ("ask", nexus_ai::core_plugin::HANDLER_ASK),
                    ("index_file", nexus_ai::core_plugin::HANDLER_INDEX_FILE),
                    (
                        "vectorstore_count",
                        nexus_ai::core_plugin::HANDLER_VECTORSTORE_COUNT,
                    ),
                    ("status", nexus_ai::core_plugin::HANDLER_STATUS),
                    ("config", nexus_ai::core_plugin::HANDLER_CONFIG),
                    (
                        "stream_chat",
                        nexus_ai::core_plugin::HANDLER_STREAM_CHAT,
                    ),
                    (
                        "stream_ask",
                        nexus_ai::core_plugin::HANDLER_STREAM_ASK,
                    ),
                    (
                        "session_load",
                        nexus_ai::core_plugin::HANDLER_SESSION_LOAD,
                    ),
                    (
                        "session_save",
                        nexus_ai::core_plugin::HANDLER_SESSION_SAVE,
                    ),
                    (
                        "session_list",
                        nexus_ai::core_plugin::HANDLER_SESSION_LIST,
                    ),
                    (
                        "session_delete",
                        nexus_ai::core_plugin::HANDLER_SESSION_DELETE,
                    ),
                    (
                        "set_config",
                        nexus_ai::core_plugin::HANDLER_SET_CONFIG,
                    ),
                    (
                        "semantic_search",
                        nexus_ai::core_plugin::HANDLER_SEMANTIC_SEARCH,
                    ),
                    // BL-041 — background indexing daemon status
                    // snapshot. Polled by the shell status badge
                    // (~2 s cadence) and surfaced through `nexus
                    // status` for headless use.
                    (
                        "index_status",
                        nexus_ai::core_plugin::HANDLER_INDEX_STATUS,
                    ),
                    // BL-045 — auto-enrichment on save. `enrich_file`
                    // proposes tags + summary + related notes for a
                    // markdown file (no write); `enrich_apply` merges
                    // a previously-returned proposal into the file's
                    // YAML frontmatter (with a body-hash drift guard).
                    (
                        "enrich_file",
                        nexus_ai::core_plugin::HANDLER_ENRICH_FILE,
                    ),
                    (
                        "enrich_apply",
                        nexus_ai::core_plugin::HANDLER_ENRICH_APPLY,
                    ),
                    // FU-2 — manual "Reindex forge" trigger. Fans
                    // every markdown file currently in the storage
                    // index onto the indexing daemon's queue. Used
                    // by the shell's status badge button + palette
                    // command.
                    (
                        "index_trigger",
                        nexus_ai::core_plugin::HANDLER_INDEX_TRIGGER,
                    ),
                    // BL-037 — per-forge AI activity timeline.
                    // `activity_list` reads the JSONL log under
                    // `.forge/ai-activity.log` newest-first;
                    // `activity_clear` truncates it.
                    (
                        "activity_list",
                        nexus_ai::core_plugin::HANDLER_ACTIVITY_LIST,
                    ),
                    (
                        "activity_clear",
                        nexus_ai::core_plugin::HANDLER_ACTIVITY_CLEAR,
                    ),
                    // G7 / ADR 0023 — single-turn provider call that
                    // returns mapped tool-use blocks without executing
                    // them. Consumed by the agent migration (Phase 1b);
                    // exposed here so the IPC contract is reachable
                    // independently of caller wiring.
                    (
                        "propose_tool_calls",
                        nexus_ai::core_plugin::HANDLER_PROPOSE_TOOL_CALLS,
                    ),
                    // BL-117 — sibling subsystems (nexus-audio) ask
                    // here for the active chat provider's credentials
                    // so the user doesn't need to configure a second
                    // API key for audio.
                    (
                        "resolve_credentials",
                        nexus_ai::core_plugin::HANDLER_RESOLVE_CREDENTIALS,
                    ),
                    // BL-116 — symbol-aware doc generator. Resolves a
                    // symbol from the BL-114 index, reads its source
                    // range, packs in parent + sibling 1-hop context
                    // (call-edges land in a follow-up BL), prompts
                    // the configured AI provider for a docblock.
                    (
                        "generate_docs",
                        nexus_ai::core_plugin::HANDLER_GENERATE_DOCS,
                    ),
                    // BL-128 close — `entity_recall`: FAISS-backed
                    // entity recall layered on the shared chunk
                    // vectorstore. Callers fall back to the
                    // substring-ranking `entity_search` when no
                    // embedder is configured.
                    (
                        "entity_recall",
                        nexus_ai::core_plugin::HANDLER_ENTITY_RECALL,
                    ),
                    (
                        "enrich_entity",
                        nexus_ai::core_plugin::HANDLER_ENRICH_ENTITY,
                    ),
                    (
                        "infer_entity_relations",
                        nexus_ai::core_plugin::HANDLER_INFER_ENTITY_RELATIONS,
                    ),
                ]),
            ),
            forge_root,
            Box::new(AiCorePlugin::new()),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.ai")?;
    Ok(())
}
