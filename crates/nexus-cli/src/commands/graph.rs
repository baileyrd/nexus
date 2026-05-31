use anyhow::Result;
use nexus_bootstrap::invoker::IpcInvoker;
use nexus_bootstrap::storage as ipc;
use tokio::runtime::Runtime as TokioRuntime;

use crate::app::App;
use crate::output::{print_list, OutputFormat};

/// Show knowledge graph statistics.
pub fn status(app: &mut App) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let stats = rt
        .block_on(ipc::graph_stats(&*invoker))
        .map_err(|e| anyhow::anyhow!("failed to get graph stats: {e}"))?;

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            println!(
                "{}",
                serde_json::json!({
                    "nodes": stats.node_count,
                    "edges": stats.edge_count,
                    "unresolved": stats.unresolved_count,
                })
            );
        }
        _ => {
            println!("Nodes      : {}", stats.node_count);
            println!("Edges      : {}", stats.edge_count);
            println!("Unresolved : {}", stats.unresolved_count);
        }
    }

    Ok(())
}

/// List all unresolved (broken) links.
pub fn unresolved(app: &mut App) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let links = rt
        .block_on(ipc::unresolved_links(&*invoker))
        .map_err(|e| anyhow::anyhow!("failed to get unresolved links: {e}"))?;

    if links.is_empty() {
        println!("No unresolved links.");
        return Ok(());
    }

    let headers = &["Target", "Referenced By"];
    let rows: Vec<Vec<String>> = links
        .iter()
        .map(|u| vec![u.target_path.clone(), u.referenced_by.join(", ")])
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// Show neighbors of a file within N hops.
pub fn neighbors(app: &mut App, path: &str, depth: usize) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let paths = rt
        .block_on(ipc::graph_neighbors(&*invoker, path, depth))
        .map_err(|e| anyhow::anyhow!("failed to get neighbors: {e}"))?;

    if paths.is_empty() {
        println!("No neighbors found.");
        return Ok(());
    }

    let headers = &["Path"];
    let rows: Vec<Vec<String>> = paths.iter().map(|p| vec![p.clone()]).collect();

    print_list(format, headers, &rows);

    Ok(())
}

// ── BL-128 entity-graph commands ──────────────────────────────────────────────

/// `nexus graph entity list` — list entities (optionally filtered).
pub fn entity_list(app: &mut App, entity_type: Option<&str>, limit: u32) -> Result<()> {
    // The `entity_search` IPC treats an empty query as "everything",
    // ordered by ascending id — reuse it for `list`.
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let hits = rt
        .block_on(ipc::entity_search(&*invoker, "", entity_type, Some(limit)))
        .map_err(|e| anyhow::anyhow!("entity_search failed: {e}"))?;

    if hits.is_empty() {
        println!("No entities.");
        return Ok(());
    }

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            for hit in &hits {
                println!(
                    "{}",
                    serde_json::json!({
                        "id": hit.id,
                        "entity_type": hit.entity_type,
                        "description": hit.description,
                        "relpath": hit.relpath,
                    })
                );
            }
        }
        _ => {
            let headers = &["Id", "Type", "Description"];
            let rows: Vec<Vec<String>> = hits
                .iter()
                .map(|h| vec![h.id.clone(), h.entity_type.clone(), h.description.clone()])
                .collect();
            print_list(format, headers, &rows);
        }
    }
    Ok(())
}

/// `nexus graph entity show <id>` — full payload + relations.
pub fn entity_show(app: &mut App, id: &str) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let entity = rt
        .block_on(ipc::entity_get(&*invoker, id))
        .map_err(|e| anyhow::anyhow!("entity_get failed: {e}"))?;
    let Some(entity) = entity else {
        return Err(anyhow::anyhow!("no entity for id or alias '{id}'"));
    };

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            println!(
                "{}",
                serde_json::json!({
                    "id": entity.id,
                    "entity_type": entity.entity_type,
                    "aliases": entity.aliases,
                    "description": entity.description,
                    "relpath": entity.relpath,
                    "relations": entity.relations.iter().map(|r| serde_json::json!({
                        "target": r.target,
                        "type": r.kind,
                        "confidence": r.confidence,
                    })).collect::<Vec<_>>(),
                })
            );
        }
        _ => {
            println!("Id          : {}", entity.id);
            println!("Type        : {}", entity.entity_type);
            if !entity.aliases.is_empty() {
                println!("Aliases     : {}", entity.aliases.join(", "));
            }
            if !entity.description.is_empty() {
                println!("Description : {}", entity.description);
            }
            println!("Relpath     : {}", entity.relpath);
            if !entity.relations.is_empty() {
                println!("Relations   :");
                for rel in &entity.relations {
                    println!(
                        "  {:<20} {:<14} (confidence {:.2})",
                        rel.target, rel.kind, rel.confidence
                    );
                }
            }
        }
    }
    Ok(())
}

/// `nexus graph entity search` — ranked substring search.
pub fn entity_search(
    app: &mut App,
    query: &str,
    entity_type: Option<&str>,
    limit: u32,
) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let hits = rt
        .block_on(ipc::entity_search(
            &*invoker,
            query,
            entity_type,
            Some(limit),
        ))
        .map_err(|e| anyhow::anyhow!("entity_search failed: {e}"))?;

    if hits.is_empty() {
        println!("No matches.");
        return Ok(());
    }

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            for hit in &hits {
                println!(
                    "{}",
                    serde_json::json!({
                        "id": hit.id,
                        "entity_type": hit.entity_type,
                        "description": hit.description,
                        "score": hit.score,
                        "relpath": hit.relpath,
                    })
                );
            }
        }
        _ => {
            let headers = &["Score", "Id", "Type", "Description"];
            let rows: Vec<Vec<String>> = hits
                .iter()
                .map(|h| {
                    vec![
                        h.score.to_string(),
                        h.id.clone(),
                        h.entity_type.clone(),
                        h.description.clone(),
                    ]
                })
                .collect();
            print_list(format, headers, &rows);
        }
    }
    Ok(())
}

/// `nexus graph entity related` — outgoing / incoming / both edges.
pub fn entity_related(app: &mut App, id: &str, direction: &str) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let edges = rt
        .block_on(ipc::entity_relations(&*invoker, id, Some(direction)))
        .map_err(|e| anyhow::anyhow!("entity_relations failed: {e}"))?;

    if edges.is_empty() {
        println!("No relations.");
        return Ok(());
    }

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            for edge in &edges {
                println!(
                    "{}",
                    serde_json::json!({
                        "from": edge.from,
                        "to": edge.to,
                        "type": edge.kind,
                        "confidence": edge.confidence,
                    })
                );
            }
        }
        _ => {
            let headers = &["From", "Type", "To", "Confidence"];
            let rows: Vec<Vec<String>> = edges
                .iter()
                .map(|e| {
                    vec![
                        e.from.clone(),
                        e.kind.clone(),
                        e.to.clone(),
                        format!("{:.2}", e.confidence),
                    ]
                })
                .collect();
            print_list(format, headers, &rows);
        }
    }
    Ok(())
}

/// `nexus graph dream-cycle run` — BL-129. Runs `dedup`, `decay`,
/// `enrich`, and `infer` in spec order. `None` runs every phase.
///
/// `dedup` auto-merges pairs at or above `merge_threshold` (default
/// `0.97`) and surfaces the remainder above `review_threshold`
/// (default `0.92`). `decay` rewrites every entity file in-place.
/// `enrich` and `infer` require a configured AI provider; they
/// short-circuit with a "skipped — no provider" message otherwise.
#[allow(clippy::too_many_arguments)]
pub fn dream_cycle_run(
    app: &mut App,
    phase: Option<&str>,
    decay_factor: Option<f32>,
    decay_floor: Option<f32>,
    review_threshold: Option<f32>,
    merge_threshold: Option<f32>,
    dry_run: bool,
) -> Result<()> {
    let run_dedup = phase.is_none_or(|p| p == "dedup");
    let run_decay = phase.is_none_or(|p| p == "decay");
    let run_enrich = phase.is_none_or(|p| p == "enrich");
    let run_infer = phase.is_none_or(|p| p == "infer");

    let format = app.format();
    let (invoker, rt) = app.invoker()?;

    if run_dedup {
        let merge_thr = merge_threshold.unwrap_or(0.97);
        // Use the lower of the two thresholds as the IPC review floor so
        // we still see merge candidates even if `--merge-threshold` is
        // below the server-side default.
        let scan_floor = review_threshold.unwrap_or(0.92).min(merge_thr);
        let pairs = rt
            .block_on(ipc::entity_find_duplicates(&*invoker, Some(scan_floor)))
            .map_err(|e| anyhow::anyhow!("dream-cycle dedup failed: {e}"))?;

        // Partition into auto-merge vs review tiers. Within each tier,
        // sort by descending similarity (already done by IPC).
        let mut merged: Vec<(String, String, f32, usize, usize)> = Vec::new();
        let mut review: Vec<(String, String, f32)> = Vec::new();
        // Track ids that already participated in a merge this pass so a
        // single entity can't be merged into two different survivors in
        // one cycle (deterministic across pair order).
        let mut consumed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for p in &pairs {
            if p.similarity >= merge_thr {
                if consumed.contains(&p.a) || consumed.contains(&p.b) {
                    // Skip — defer to next cycle when one side reappears.
                    continue;
                }
                if dry_run {
                    merged.push((p.a.clone(), p.b.clone(), p.similarity, 0, 0));
                    consumed.insert(p.b.clone());
                    continue;
                }
                let outcome = rt
                    .block_on(ipc::entity_merge(&*invoker, &p.a, &p.b))
                    .map_err(|e| {
                        anyhow::anyhow!("dream-cycle dedup: merge {} ← {} failed: {e}", p.a, p.b)
                    })?;
                consumed.insert(outcome.dropped.clone());
                merged.push((
                    outcome.kept,
                    outcome.dropped,
                    p.similarity,
                    outcome.aliases_added as usize,
                    outcome.relations_added as usize,
                ));
            } else {
                review.push((p.a.clone(), p.b.clone(), p.similarity));
            }
        }

        match format {
            OutputFormat::Json | OutputFormat::Jsonl => {
                println!(
                    "{}",
                    serde_json::json!({
                        "phase": "dedup",
                        "merged": merged.iter().map(|(k, d, s, a, r)| serde_json::json!({
                            "kept": k,
                            "dropped": d,
                            "similarity": s,
                            "aliases_added": a,
                            "relations_added": r,
                        })).collect::<Vec<_>>(),
                        "review": review.iter().map(|(a, b, s)| serde_json::json!({
                            "a": a,
                            "b": b,
                            "similarity": s,
                        })).collect::<Vec<_>>(),
                        "dry_run": dry_run,
                    })
                );
            }
            _ => {
                let mode = if dry_run { " (dry-run)" } else { "" };
                println!(
                    "dedup    : {m} merged, {r} for review (merge≥{mt:.2}, review≥{rt:.2}){mode}",
                    m = merged.len(),
                    r = review.len(),
                    mt = merge_thr,
                    rt = review_threshold.unwrap_or(0.92),
                );
                for (kept, dropped, sim, aliases, relations) in &merged {
                    println!(
                        "  merge {sim:.3}  {kept} ← {dropped}  (+{aliases} aliases, +{relations} relations)"
                    );
                }
                for (a, b, sim) in &review {
                    println!("  review {sim:.3}  {a}  {b}");
                }
            }
        }
    }

    if run_decay {
        let outcome = rt
            .block_on(ipc::entity_decay_relations(
                &*invoker,
                decay_factor,
                decay_floor,
                dry_run,
            ))
            .map_err(|e| anyhow::anyhow!("dream-cycle decay failed: {e}"))?;
        match format {
            OutputFormat::Json | OutputFormat::Jsonl => {
                println!(
                    "{}",
                    serde_json::json!({
                        "phase": "decay",
                        "entities_scanned":   outcome.entities_scanned,
                        "entities_updated":   outcome.entities_updated,
                        "relations_decayed":  outcome.relations_decayed,
                        "relations_at_floor": outcome.relations_at_floor,
                        "dry_run":            outcome.dry_run,
                    })
                );
            }
            _ => {
                let mode = if outcome.dry_run { " (dry-run)" } else { "" };
                println!(
                    "decay    : scanned {scanned}, updated {updated}, decayed {decayed}, at-floor {floor}{mode}",
                    scanned = outcome.entities_scanned,
                    updated = outcome.entities_updated,
                    decayed = outcome.relations_decayed,
                    floor   = outcome.relations_at_floor,
                );
            }
        }
    }

    if run_enrich || run_infer {
        // Both phases iterate over every entity, so list once.
        let entity_ids = list_entity_ids(&*invoker, rt)?;

        if run_enrich {
            let mut enriched = 0u32;
            let mut skipped = 0u32;
            let mut failures: Vec<String> = Vec::new();
            for id in &entity_ids {
                let args = serde_json::json!({
                    "entity_id": id,
                    "dry_run": dry_run,
                });
                match ai_ipc_call(&*invoker, rt, "enrich_entity", args) {
                    Ok(reply) => {
                        if reply
                            .get("skipped")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false)
                        {
                            skipped += 1;
                        } else if reply
                            .get("applied")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false)
                            || dry_run
                        {
                            enriched += 1;
                        }
                    }
                    Err(e) => failures.push(format!("{id}: {e}")),
                }
            }
            match format {
                OutputFormat::Json | OutputFormat::Jsonl => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "phase": "enrich",
                            "enriched": enriched,
                            "skipped":  skipped,
                            "failed":   failures.len(),
                            "dry_run":  dry_run,
                        })
                    );
                }
                _ => {
                    let mode = if dry_run { " (dry-run)" } else { "" };
                    println!(
                        "enrich   : enriched {enriched}, skipped {skipped}, failed {failed}{mode}",
                        failed = failures.len(),
                    );
                    if let Some(first) = failures.first() {
                        println!("  first failure: {first}");
                    }
                }
            }
        }

        if run_infer {
            let mut proposals_total = 0u32;
            let mut entities_with_proposals = 0u32;
            let mut failures: Vec<String> = Vec::new();
            for id in &entity_ids {
                let args = serde_json::json!({
                    "entity_id": id,
                    "dry_run":   dry_run,
                });
                match ai_ipc_call(&*invoker, rt, "infer_entity_relations", args) {
                    Ok(reply) => {
                        let n = reply
                            .get("proposals")
                            .and_then(serde_json::Value::as_array)
                            .map_or(0, Vec::len) as u32;
                        if n > 0 {
                            entities_with_proposals += 1;
                            proposals_total += n;
                        }
                    }
                    Err(e) => failures.push(format!("{id}: {e}")),
                }
            }
            match format {
                OutputFormat::Json | OutputFormat::Jsonl => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "phase": "infer",
                            "entities_with_proposals": entities_with_proposals,
                            "proposals_total":         proposals_total,
                            "failed":                  failures.len(),
                            "dry_run":                 dry_run,
                        })
                    );
                }
                _ => {
                    let mode = if dry_run { " (dry-run)" } else { "" };
                    println!(
                        "infer    : {proposals_total} new proposal(s) across {entities_with_proposals} \
                         entity/-ies, failed {failed}{mode}",
                        failed = failures.len(),
                    );
                    if let Some(first) = failures.first() {
                        println!("  first failure: {first}");
                    }
                }
            }
        }
    }

    Ok(())
}

/// List every canonical entity id in the forge. Uses the empty-query
/// path of `entity_search` (returns everything ordered by id ascending,
/// up to `limit`). The 5000 cap is a defensive ceiling — the dream
/// cycle is intended for personal forges with O(hundreds) of entities.
fn list_entity_ids(
    invoker: &(dyn IpcInvoker + Send + Sync),
    rt: &TokioRuntime,
) -> Result<Vec<String>> {
    let hits = rt
        .block_on(ipc::entity_search(invoker, "", None, Some(5000)))
        .map_err(|e| anyhow::anyhow!("dream-cycle: list entities failed: {e}"))?;
    Ok(hits.into_iter().map(|h| h.id).collect())
}

/// Direct IPC call into `com.nexus.ai`. The bootstrap crate exposes
/// typed helpers for storage but not for AI, so this is the narrowest
/// adapter we need for the dream-cycle CLI.
fn ai_ipc_call(
    invoker: &(dyn IpcInvoker + Send + Sync),
    rt: &TokioRuntime,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value> {
    rt.block_on(invoker.ipc_call(
        nexus_types::plugin_ids::AI,
        command,
        args,
        nexus_types::constants::IPC_TIMEOUT_LONG,
    ))
    .map_err(|e| anyhow::anyhow!("AI ipc call '{command}' failed: {e}"))
}

/// `nexus graph entity duplicates` — Jaccard-similar entity pairs.
pub fn entity_duplicates(app: &mut App, threshold: f32) -> Result<()> {
    let format = app.format();
    let (invoker, rt) = app.invoker()?;
    let pairs = rt
        .block_on(ipc::entity_find_duplicates(&*invoker, Some(threshold)))
        .map_err(|e| anyhow::anyhow!("entity_find_duplicates failed: {e}"))?;

    if pairs.is_empty() {
        println!("No duplicate candidates above threshold {threshold:.2}.");
        return Ok(());
    }

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            for pair in &pairs {
                println!(
                    "{}",
                    serde_json::json!({
                        "a": pair.a,
                        "b": pair.b,
                        "similarity": pair.similarity,
                    })
                );
            }
        }
        _ => {
            let headers = &["Similarity", "A", "B"];
            let rows: Vec<Vec<String>> = pairs
                .iter()
                .map(|p| vec![format!("{:.3}", p.similarity), p.a.clone(), p.b.clone()])
                .collect();
            print_list(format, headers, &rows);
        }
    }
    Ok(())
}
