//! Entity-index handlers: `entity_search`, `entity_get`,
//! `entity_relations`, `entity_upsert`, `entity_find_duplicates`,
//! `entity_merge`, `entity_decay_relations`, `list_draft_relations`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use super::shared::{exec_err, parse_args, to_value};

pub(crate) fn search(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let parsed: crate::ipc::EntitySearchArgs = parse_args(args, "entity_search")?;
    let limit = parsed.limit.unwrap_or(10).max(1) as usize;
    let index = crate::entity_index::EntityIndex::load(forge_root);
    let hits = index.search(&parsed.query, parsed.entity_type.as_deref(), limit);
    let result = crate::ipc::EntitySearchResult {
        results: hits
            .into_iter()
            .map(|h| crate::ipc::EntitySearchHitRow {
                id: h.id,
                entity_type: h.entity_type,
                description: h.description,
                relpath: h.relpath,
                score: h.score,
            })
            .collect(),
    };
    to_value(&result, "entity_search")
}

pub(crate) fn get(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let parsed: crate::ipc::EntityGetArgs = parse_args(args, "entity_get")?;
    let index = crate::entity_index::EntityIndex::load(forge_root);
    let entity = index.get(&parsed.id).map(|rec| crate::ipc::EntityRecordRow {
        id: rec.id.clone(),
        entity_type: rec.entity_type.clone(),
        aliases: rec.aliases.clone(),
        description: rec.description.clone(),
        relations: rec
            .relations
            .iter()
            .map(|r| crate::ipc::EntityRelationRow {
                target: r.target.clone(),
                kind: r.kind.clone(),
                confidence: r.confidence,
            })
            .collect(),
        relpath: rec.relpath.clone(),
    });
    to_value(&crate::ipc::EntityGetResult { entity }, "entity_get")
}

pub(crate) fn relations(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let parsed: crate::ipc::EntityRelationsArgs = parse_args(args, "entity_relations")?;
    let direction = crate::entity_index::RelationDirection::parse(parsed.direction.as_deref());
    let index = crate::entity_index::EntityIndex::load(forge_root);
    let relations = index
        .relations(&parsed.id, direction)
        .into_iter()
        .map(|r| crate::ipc::EntityRelationsResultRow {
            from: r.from,
            to: r.to,
            kind: r.kind,
            confidence: r.confidence,
        })
        .collect();
    to_value(
        &crate::ipc::EntityRelationsResult { relations },
        "entity_relations",
    )
}

pub(crate) fn upsert(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let parsed: crate::ipc::EntityUpsertArgs = parse_args(args, "entity_upsert")?;
    let id_trimmed = parsed.id.trim();
    if id_trimmed.is_empty() {
        return Err(exec_err("entity_upsert: 'id' must be non-empty".to_string()));
    }
    if id_trimmed.contains(['/', '\\']) || id_trimmed.contains("..") {
        return Err(exec_err(
            "entity_upsert: 'id' must be a bare file stem (no path separators or '..')"
                .to_string(),
        ));
    }
    let entity_type_trimmed = parsed.entity_type.trim();
    if entity_type_trimmed.is_empty() {
        return Err(exec_err(
            "entity_upsert: 'entity_type' must be non-empty".to_string(),
        ));
    }
    let payload = crate::entity_index::EntityUpsert {
        id: id_trimmed.to_string(),
        entity_type: entity_type_trimmed.to_string(),
        aliases: parsed
            .aliases
            .into_iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect(),
        description: parsed.description.trim().to_string(),
        relations: parsed
            .relations
            .into_iter()
            .map(|r| crate::entity_index::EntityUpsertRelation {
                target: r.target,
                kind: r.kind,
                confidence: r.confidence,
            })
            .collect(),
    };
    let markdown = crate::entity_index::render_entity_markdown(&payload);
    let target = forge_root
        .join(crate::entity_index::ENTITIES_DIR)
        .join(format!("{}.md", payload.id));
    let replaced = target.exists();
    let temp_dir = forge_root.join(".forge").join("temp");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| exec_err(format!("entity_upsert: create temp dir: {e}")))?;
    crate::atomic_write(&target, markdown.as_bytes(), &temp_dir)
        .map_err(|e| exec_err(format!("entity_upsert: write {}: {e}", target.display())))?;
    let result = crate::ipc::EntityUpsertResult {
        relpath: format!("{}/{}.md", crate::entity_index::ENTITIES_DIR, payload.id),
        replaced,
    };
    to_value(&result, "entity_upsert")
}

pub(crate) fn find_duplicates(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let parsed: crate::ipc::EntityFindDuplicatesArgs =
        parse_args(args, "entity_find_duplicates")?;
    let threshold = parsed.threshold.unwrap_or(0.92).clamp(0.0, 1.0);
    let index = crate::entity_index::EntityIndex::load(forge_root);
    let pairs = index
        .find_duplicates(threshold)
        .into_iter()
        .map(|p| crate::ipc::EntityDuplicatePairRow {
            a: p.a,
            b: p.b,
            similarity: p.similarity,
        })
        .collect();
    to_value(
        &crate::ipc::EntityFindDuplicatesResult { pairs },
        "entity_find_duplicates",
    )
}

pub(crate) fn merge(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let parsed: crate::ipc::EntityMergeArgs = parse_args(args, "entity_merge")?;
    let keep_id = parsed.keep.trim().to_string();
    let drop_id = parsed.drop.trim().to_string();
    if keep_id.is_empty() || drop_id.is_empty() {
        return Err(exec_err(
            "entity_merge: 'keep' and 'drop' must both be non-empty".to_string(),
        ));
    }
    if keep_id == drop_id {
        return Err(exec_err(
            "entity_merge: 'keep' and 'drop' must differ".to_string(),
        ));
    }
    for id in [&keep_id, &drop_id] {
        if id.contains(['/', '\\']) || id.contains("..") {
            return Err(exec_err(
                "entity_merge: ids must be bare file stems (no path separators or '..')"
                    .to_string(),
            ));
        }
    }

    let index = crate::entity_index::EntityIndex::load(forge_root);
    let Some(keep_rec) = index.get(&keep_id).cloned() else {
        return Err(exec_err(format!(
            "entity_merge: 'keep' entity '{keep_id}' not found"
        )));
    };
    // `drop` may resolve via alias — refuse alias-only drops because the
    // delete needs a concrete file path.
    let Some(drop_rec) = index.get(&drop_id).cloned() else {
        return Err(exec_err(format!(
            "entity_merge: 'drop' entity '{drop_id}' not found"
        )));
    };
    if drop_rec.id != drop_id {
        return Err(exec_err(format!(
            "entity_merge: 'drop' must be a canonical id (got alias for '{}')",
            drop_rec.id
        )));
    }
    if keep_rec.id == drop_rec.id {
        return Err(exec_err(
            "entity_merge: 'keep' and 'drop' resolved to the same entity".to_string(),
        ));
    }

    let merged = crate::entity_index::merge_records(&keep_rec, &drop_rec);

    let target = forge_root
        .join(crate::entity_index::ENTITIES_DIR)
        .join(format!("{}.md", keep_rec.id));
    let temp_dir = forge_root.join(".forge").join("temp");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| exec_err(format!("entity_merge: create temp dir: {e}")))?;
    let markdown = crate::entity_index::render_entity_markdown(&merged.payload);
    crate::atomic_write(&target, markdown.as_bytes(), &temp_dir).map_err(|e| {
        exec_err(format!("entity_merge: write {}: {e}", target.display()))
    })?;

    let drop_path = forge_root
        .join(crate::entity_index::ENTITIES_DIR)
        .join(format!("{}.md", drop_rec.id));
    if drop_path.exists() {
        std::fs::remove_file(&drop_path).map_err(|e| {
            exec_err(format!(
                "entity_merge: remove {}: {e}",
                drop_path.display()
            ))
        })?;
    }

    let result = crate::ipc::EntityMergeResult {
        kept:            keep_rec.id,
        dropped:         drop_rec.id,
        aliases_added:   merged.aliases_added,
        relations_added: merged.relations_added,
    };
    to_value(&result, "entity_merge")
}

pub(crate) fn list_draft_relations(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let parsed: crate::ipc::ListDraftRelationsArgs =
        parse_args(args, "list_draft_relations")?;
    let threshold = parsed.threshold.unwrap_or(0.5);
    let limit = parsed
        .limit
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(200)
        .max(1);
    let index = crate::entity_index::EntityIndex::load(forge_root);
    let (rows, total) = index.list_draft_relations(threshold, limit);
    let truncated = (rows.len() as u32) < total;
    let result = crate::ipc::ListDraftRelationsResult {
        relations: rows
            .into_iter()
            .map(|r| crate::ipc::DraftRelationRow {
                from:       r.from,
                target:     r.target,
                kind:       r.kind,
                confidence: r.confidence,
                relpath:    r.relpath,
            })
            .collect(),
        total,
        truncated,
    };
    to_value(&result, "list_draft_relations")
}

pub(crate) fn decay_relations(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    let parsed: crate::ipc::EntityDecayRelationsArgs =
        parse_args(args, "entity_decay_relations")?;
    let params = crate::entity_index::DecayParams {
        factor: parsed.factor.unwrap_or(0.95),
        floor:  parsed.floor.unwrap_or(0.10),
    };
    let dry_run = parsed.dry_run.unwrap_or(false);

    let entities_dir = forge_root.join(crate::entity_index::ENTITIES_DIR);
    let mut result = crate::ipc::EntityDecayRelationsResult {
        dry_run,
        ..Default::default()
    };
    let Ok(read_dir) = std::fs::read_dir(&entities_dir) else {
        // Missing entities/ dir is not an error — return zero counts so
        // the dream cycle CLI prints "no entities" cleanly.
        return to_value(&result, "entity_decay_relations");
    };

    let temp_dir = forge_root.join(".forge").join("temp");
    if !dry_run {
        std::fs::create_dir_all(&temp_dir).map_err(|e| {
            exec_err(format!("entity_decay_relations: create temp dir: {e}"))
        })?;
    }

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_md = path.extension().and_then(|s| s.to_str()).is_some_and(|ext| {
            ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
        });
        if !is_md {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        result.entities_scanned += 1;
        let Some(decayed) = crate::entity_index::decay_file_content(&content, &params) else {
            continue;
        };
        result.entities_updated += 1;
        result.relations_decayed += decayed.relations_decayed;
        result.relations_at_floor += decayed.relations_at_floor;
        if dry_run {
            continue;
        }
        crate::atomic_write(&path, decayed.content.as_bytes(), &temp_dir).map_err(|e| {
            exec_err(format!(
                "entity_decay_relations: write {}: {e}",
                path.display()
            ))
        })?;
    }
    to_value(&result, "entity_decay_relations")
}
