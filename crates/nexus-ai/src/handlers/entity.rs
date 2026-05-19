//! BL-129 — entity enrichment + relation inference handlers.

use nexus_kernel::{Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;

use crate::config::AiConfig;
use crate::handlers::shared::{build_ai_provider, build_embedding_provider, exec_err, extract_json_array};
use crate::rag;

const ENRICH_ENTITY_MAX_RAG_HITS: usize = 4;
const ENRICH_ENTITY_MAX_CHUNK_CHARS: usize = 400;
const ENRICH_ENTITY_MAX_DESCRIPTION: usize = 480;

pub(crate) async fn handle_enrich_entity(
    ctx: &KernelPluginContext,
    ai_cfg: Option<AiConfig>,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    use crate::ipc::{EnrichEntityArgs, EnrichEntityResult};
    use crate::provider::{ChatMessage, Role};

    let parsed: EnrichEntityArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("enrich_entity: parse args: {e}")))?;
    let entity_arg = parsed.entity_id.trim();
    if entity_arg.is_empty() {
        return Err(exec_err("enrich_entity: 'entity_id' must be non-empty".to_string()));
    }
    let min_chars = parsed.min_description_chars.unwrap_or(80) as usize;
    let dry_run = parsed.dry_run.unwrap_or(false);

    // ── 1. Look up the entity through storage ─────────────────────────────
    let entity_resp = ctx
        .ipc_call(
            "com.nexus.storage",
            "entity_get",
            serde_json::json!({ "id": entity_arg }),
            std::time::Duration::from_secs(10),
        )
        .await
        .map_err(|e| exec_err(format!("enrich_entity: entity_get: {e}")))?;
    let entity_obj = entity_resp
        .get("entity")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| exec_err(format!("enrich_entity: '{entity_arg}' not found")))?;
    let entity_id = entity_obj
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(entity_arg)
        .to_string();
    let entity_type = entity_obj
        .get("entity_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("entity")
        .to_string();
    let aliases: Vec<String> = entity_obj
        .get("aliases")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let original_description = entity_obj
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let existing_relations: Vec<(String, String, f32)> = entity_obj
        .get("relations")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let t = v.get("target").and_then(serde_json::Value::as_str)?;
                    let k = v.get("type").and_then(serde_json::Value::as_str)?;
                    let c = v
                        .get("confidence")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(1.0) as f32;
                    Some((t.to_string(), k.to_string(), c))
                })
                .collect()
        })
        .unwrap_or_default();

    // Short-circuit when the description is already substantial.
    if original_description.chars().count() >= min_chars {
        return serde_json::to_value(&EnrichEntityResult {
            entity_id,
            original_description,
            new_description: String::new(),
            skipped: true,
            applied: false,
        })
        .map_err(|e| exec_err(format!("enrich_entity: serialise: {e}")));
    }

    // ── 2. Optional RAG context (only when an embedder is configured) ────
    let mut rag_snippets: Vec<String> = Vec::new();
    if let Some(embed_cfg) = embed_cfg {
        if let Ok(embedder) = build_embedding_provider(&embed_cfg) {
            let query = format!("{entity_id} {}", aliases.join(" "));
            if let Ok(matches) =
                rag::semantic_search(ctx, embedder.as_ref(), query.trim(), 12).await
            {
                for hit in matches {
                    if hit.file_path.starts_with("entities/") {
                        continue;
                    }
                    let mut snippet = hit.chunk_text.clone();
                    if snippet.chars().count() > ENRICH_ENTITY_MAX_CHUNK_CHARS {
                        snippet = snippet
                            .chars()
                            .take(ENRICH_ENTITY_MAX_CHUNK_CHARS)
                            .collect::<String>()
                            + "…";
                    }
                    rag_snippets.push(format!("- {}: {snippet}", hit.file_path));
                    if rag_snippets.len() >= ENRICH_ENTITY_MAX_RAG_HITS {
                        break;
                    }
                }
            }
        }
    }

    // ── 3. AI call — require a chat provider ─────────────────────────────
    let ai_cfg = ai_cfg.ok_or_else(|| {
        exec_err("enrich_entity: no AI chat provider configured".to_string())
    })?;
    let provider = build_ai_provider(&ai_cfg).map_err(exec_err)?;

    let mut prompt = String::new();
    prompt.push_str("You are expanding a brief knowledge-graph entity description into a single self-contained paragraph (max 3 sentences, no preface, no bullet points).\n\n");
    prompt.push_str(&format!("Entity: {entity_id}\n"));
    prompt.push_str(&format!("Type: {entity_type}\n"));
    if !aliases.is_empty() {
        prompt.push_str(&format!("Aliases: {}\n", aliases.join(", ")));
    }
    prompt.push_str(&format!(
        "Existing description: {}\n",
        if original_description.is_empty() { "(none)" } else { &original_description }
    ));
    if !existing_relations.is_empty() {
        prompt.push_str("Existing relations:\n");
        for (target, kind, _) in &existing_relations {
            prompt.push_str(&format!("- {kind} {target}\n"));
        }
    }
    if !rag_snippets.is_empty() {
        prompt.push_str("\nSupporting snippets from notes:\n");
        for s in &rag_snippets {
            prompt.push_str(s);
            prompt.push('\n');
        }
    }
    prompt.push_str("\nReply with only the expanded description text.");

    let messages = [ChatMessage {
        role: Role::User,
        content: prompt,
    }];
    let mut new_description = provider
        .chat(&messages, None)
        .await
        .map_err(|e| exec_err(format!("enrich_entity: provider chat: {e}")))?
        .trim()
        .to_string();
    if new_description.chars().count() > ENRICH_ENTITY_MAX_DESCRIPTION {
        new_description = new_description
            .chars()
            .take(ENRICH_ENTITY_MAX_DESCRIPTION)
            .collect::<String>()
            + "…";
    }
    if new_description.is_empty() {
        return serde_json::to_value(&EnrichEntityResult {
            entity_id,
            original_description,
            new_description,
            skipped: false,
            applied: false,
        })
        .map_err(|e| exec_err(format!("enrich_entity: serialise: {e}")));
    }

    // ── 4. Write back unless dry_run ─────────────────────────────────────
    let mut applied = false;
    if !dry_run {
        let relations_payload: Vec<serde_json::Value> = existing_relations
            .iter()
            .map(|(target, kind, confidence)| {
                serde_json::json!({
                    "target": target,
                    "type":   kind,
                    "confidence": confidence,
                })
            })
            .collect();
        let upsert_args = serde_json::json!({
            "id":          entity_id,
            "entity_type": entity_type,
            "aliases":     aliases,
            "description": new_description,
            "relations":   relations_payload,
        });
        ctx.ipc_call(
            "com.nexus.storage",
            "entity_upsert",
            upsert_args,
            std::time::Duration::from_secs(10),
        )
        .await
        .map_err(|e| exec_err(format!("enrich_entity: entity_upsert: {e}")))?;
        applied = true;
    }

    serde_json::to_value(&EnrichEntityResult {
        entity_id,
        original_description,
        new_description,
        skipped: false,
        applied,
    })
    .map_err(|e| exec_err(format!("enrich_entity: serialise: {e}")))
}

// ── BL-129 close — infer_entity_relations ───────────────────────────────────

const INFER_DEFAULT_MAX_PROPOSALS: u32 = 3;
const INFER_NEIGHBOUR_FAN_OUT: usize = 6;
const INFER_DRAFT_CONFIDENCE: f32 = 0.5;

pub(crate) async fn handle_infer_entity_relations(
    ctx: &KernelPluginContext,
    ai_cfg: Option<AiConfig>,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    use crate::ipc::{
        InferEntityRelationsArgs, InferEntityRelationsResult, InferredRelationRow,
    };
    use crate::provider::{ChatMessage, Role};

    let parsed: InferEntityRelationsArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("infer_entity_relations: parse args: {e}")))?;
    let entity_arg = parsed.entity_id.trim();
    if entity_arg.is_empty() {
        return Err(exec_err(
            "infer_entity_relations: 'entity_id' must be non-empty".to_string(),
        ));
    }
    let max_proposals = parsed
        .max_proposals
        .unwrap_or(INFER_DEFAULT_MAX_PROPOSALS)
        .max(1) as usize;
    let dry_run = parsed.dry_run.unwrap_or(false);

    // ── 1. Look up the source entity ─────────────────────────────────────
    let entity_resp = ctx
        .ipc_call(
            "com.nexus.storage",
            "entity_get",
            serde_json::json!({ "id": entity_arg }),
            std::time::Duration::from_secs(10),
        )
        .await
        .map_err(|e| exec_err(format!("infer_entity_relations: entity_get: {e}")))?;
    let entity_obj = entity_resp
        .get("entity")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            exec_err(format!(
                "infer_entity_relations: '{entity_arg}' not found"
            ))
        })?;
    let entity_id = entity_obj
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(entity_arg)
        .to_string();
    let entity_type = entity_obj
        .get("entity_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("entity")
        .to_string();
    let description = entity_obj
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let existing_relations: Vec<(String, String, f32)> = entity_obj
        .get("relations")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let t = v.get("target").and_then(serde_json::Value::as_str)?;
                    let k = v.get("type").and_then(serde_json::Value::as_str)?;
                    let c = v
                        .get("confidence")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(1.0) as f32;
                    Some((t.to_string(), k.to_string(), c))
                })
                .collect()
        })
        .unwrap_or_default();

    let existing_keys: std::collections::BTreeSet<(String, String)> = existing_relations
        .iter()
        .map(|(t, k, _)| (t.clone(), k.clone()))
        .collect();

    // ── 2. Gather similar entities via recall + fall back to listing ─────
    let mut neighbours: Vec<(String, String, String)> = Vec::new();
    if let Some(embed_cfg) = embed_cfg {
        if let Ok(embedder) = build_embedding_provider(&embed_cfg) {
            let query = format!("{entity_id} {description}");
            if let Ok(matches) =
                rag::semantic_search(ctx, embedder.as_ref(), query.trim(), 20).await
            {
                let mut seen: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                seen.insert(entity_id.clone());
                for hit in matches {
                    if !hit.file_path.starts_with("entities/") {
                        continue;
                    }
                    let stem = std::path::Path::new(&hit.file_path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(str::to_string)
                        .unwrap_or_default();
                    if stem.is_empty() || !seen.insert(stem.clone()) {
                        continue;
                    }
                    if let Ok(resp) = ctx
                        .ipc_call(
                            "com.nexus.storage",
                            "entity_get",
                            serde_json::json!({ "id": stem }),
                            std::time::Duration::from_secs(5),
                        )
                        .await
                    {
                        if let Some(obj) = resp.get("entity").and_then(serde_json::Value::as_object) {
                            let id = obj
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or(&stem)
                                .to_string();
                            let etype = obj
                                .get("entity_type")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("entity")
                                .to_string();
                            let edesc = obj
                                .get("description")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            neighbours.push((id, etype, edesc));
                            if neighbours.len() >= INFER_NEIGHBOUR_FAN_OUT {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    if neighbours.is_empty() {
        if let Ok(resp) = ctx
            .ipc_call(
                "com.nexus.storage",
                "entity_search",
                serde_json::json!({ "query": "", "limit": (INFER_NEIGHBOUR_FAN_OUT + 1) as u32 }),
                std::time::Duration::from_secs(5),
            )
            .await
        {
            if let Some(arr) = resp.get("results").and_then(serde_json::Value::as_array) {
                for v in arr {
                    let id = v
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if id.is_empty() || id == entity_id {
                        continue;
                    }
                    let etype = v
                        .get("entity_type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("entity")
                        .to_string();
                    let edesc = v
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    neighbours.push((id, etype, edesc));
                    if neighbours.len() >= INFER_NEIGHBOUR_FAN_OUT {
                        break;
                    }
                }
            }
        }
    }
    if neighbours.is_empty() {
        return serde_json::to_value(&InferEntityRelationsResult {
            entity_id,
            proposals: Vec::new(),
            applied: false,
        })
        .map_err(|e| exec_err(format!("infer_entity_relations: serialise: {e}")));
    }

    // ── 3. Prompt the model for proposals (JSON-only reply) ──────────────
    let ai_cfg = ai_cfg.ok_or_else(|| {
        exec_err("infer_entity_relations: no AI chat provider configured".to_string())
    })?;
    let provider = build_ai_provider(&ai_cfg).map_err(exec_err)?;

    let mut prompt = String::new();
    prompt.push_str("You propose new typed relations between knowledge-graph entities.\n\n");
    prompt.push_str(&format!(
        "Source entity:\n  id: {entity_id}\n  type: {entity_type}\n  description: {desc}\n",
        desc = if description.is_empty() { "(none)".to_string() } else { description.clone() },
    ));
    if !existing_relations.is_empty() {
        prompt.push_str("  existing relations:\n");
        for (target, kind, _) in &existing_relations {
            prompt.push_str(&format!("    - {kind} {target}\n"));
        }
    }
    prompt.push_str("\nCandidate target entities:\n");
    for (id, etype, edesc) in &neighbours {
        prompt.push_str(&format!(
            "  - id: {id}\n    type: {etype}\n    description: {}\n",
            if edesc.is_empty() { "(none)".to_string() } else { edesc.clone() },
        ));
    }
    prompt.push_str(&format!(
        "\nReply with a JSON array of at most {max_proposals} new relation proposals. Each item: {{\"target\": <id>, \"type\": <relation kind>}}. Use only target ids from the candidate list. Skip relations that are already declared on the source. Do not include any text outside the JSON array."
    ));

    let messages = [ChatMessage {
        role: Role::User,
        content: prompt,
    }];
    let raw_reply = provider
        .chat(&messages, None)
        .await
        .map_err(|e| exec_err(format!("infer_entity_relations: provider chat: {e}")))?;

    let parsed_array: Vec<serde_json::Value> = extract_json_array(&raw_reply).unwrap_or_default();

    let valid_targets: std::collections::BTreeSet<&str> =
        neighbours.iter().map(|(id, _, _)| id.as_str()).collect();
    let mut proposals: Vec<InferredRelationRow> = Vec::new();
    let mut chosen_keys: std::collections::BTreeSet<(String, String)> =
        existing_keys.clone();
    for item in parsed_array {
        let target = match item.get("target").and_then(serde_json::Value::as_str) {
            Some(t) => t.trim(),
            None => continue,
        };
        let kind = match item.get("type").and_then(serde_json::Value::as_str) {
            Some(k) => k.trim(),
            None => continue,
        };
        if !valid_targets.contains(target) || target == entity_id || kind.is_empty() {
            continue;
        }
        let canonical = kind
            .to_ascii_lowercase()
            .replace([' ', '-'], "_");
        let key = (target.to_string(), canonical.clone());
        if !chosen_keys.insert(key.clone()) {
            continue;
        }
        proposals.push(InferredRelationRow {
            target:     target.to_string(),
            kind:       canonical,
            confidence: INFER_DRAFT_CONFIDENCE,
        });
        if proposals.len() >= max_proposals {
            break;
        }
    }

    // ── 4. Write back unless dry_run ─────────────────────────────────────
    let mut applied = false;
    if !dry_run && !proposals.is_empty() {
        let mut relations_payload: Vec<serde_json::Value> = existing_relations
            .iter()
            .map(|(t, k, c)| {
                serde_json::json!({
                    "target": t,
                    "type":   k,
                    "confidence": c,
                })
            })
            .collect();
        for p in &proposals {
            relations_payload.push(serde_json::json!({
                "target":     p.target,
                "type":       p.kind,
                "confidence": p.confidence,
            }));
        }
        let aliases: Vec<String> = entity_obj
            .get("aliases")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let upsert_args = serde_json::json!({
            "id":          entity_id,
            "entity_type": entity_type,
            "aliases":     aliases,
            "description": description,
            "relations":   relations_payload,
        });
        ctx.ipc_call(
            "com.nexus.storage",
            "entity_upsert",
            upsert_args,
            std::time::Duration::from_secs(10),
        )
        .await
        .map_err(|e| exec_err(format!("infer_entity_relations: entity_upsert: {e}")))?;
        applied = true;
    }

    serde_json::to_value(&InferEntityRelationsResult {
        entity_id,
        proposals,
        applied,
    })
    .map_err(|e| exec_err(format!("infer_entity_relations: serialise: {e}")))
}
