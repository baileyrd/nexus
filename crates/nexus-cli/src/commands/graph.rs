use anyhow::Result;
use nexus_bootstrap::storage as ipc;

use crate::app::App;
use crate::output::{print_list, OutputFormat};

/// Show knowledge graph statistics.
pub fn status(app: &mut App) -> Result<()> {
    let format = app.format();
    let (runtime, rt) = app.runtime()?;
    let stats = ipc::graph_stats(runtime, rt)
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
    let (runtime, rt) = app.runtime()?;
    let links = ipc::unresolved_links(runtime, rt)
        .map_err(|e| anyhow::anyhow!("failed to get unresolved links: {e}"))?;

    if links.is_empty() {
        println!("No unresolved links.");
        return Ok(());
    }

    let headers = &["Target", "Referenced By"];
    let rows: Vec<Vec<String>> = links
        .iter()
        .map(|u| {
            vec![
                u.target_path.clone(),
                u.referenced_by.join(", "),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// Show neighbors of a file within N hops.
pub fn neighbors(app: &mut App, path: &str, depth: usize) -> Result<()> {
    let format = app.format();
    let (runtime, rt) = app.runtime()?;
    let paths = ipc::graph_neighbors(runtime, rt, path, depth)
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
    let (runtime, rt) = app.runtime()?;
    let hits = ipc::entity_search(runtime, rt, "", entity_type, Some(limit))
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
                .map(|h| {
                    vec![
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

/// `nexus graph entity show <id>` — full payload + relations.
pub fn entity_show(app: &mut App, id: &str) -> Result<()> {
    let format = app.format();
    let (runtime, rt) = app.runtime()?;
    let entity = ipc::entity_get(runtime, rt, id)
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
    let (runtime, rt) = app.runtime()?;
    let hits = ipc::entity_search(runtime, rt, query, entity_type, Some(limit))
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
    let (runtime, rt) = app.runtime()?;
    let edges = ipc::entity_relations(runtime, rt, id, Some(direction))
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

/// `nexus graph dream-cycle run` — BL-129 thin slice. Runs the
/// `dedup` and/or `decay` phases (close-out adds `enrich` + `infer`).
///
/// Phase ordering matches the spec: dedup → decay. `None` runs every
/// supported phase. All thresholds / factors fall back to server-side
/// defaults when not supplied.
pub fn dream_cycle_run(
    app: &mut App,
    phase: Option<&str>,
    decay_factor: Option<f32>,
    decay_floor: Option<f32>,
    review_threshold: Option<f32>,
    dry_run: bool,
) -> Result<()> {
    let run_dedup = phase.is_none_or(|p| p == "dedup");
    let run_decay = phase.is_none_or(|p| p == "decay");

    let format = app.format();
    let (runtime, rt) = app.runtime()?;

    if run_dedup {
        let pairs = ipc::entity_find_duplicates(runtime, rt, review_threshold)
            .map_err(|e| anyhow::anyhow!("dream-cycle dedup failed: {e}"))?;
        match format {
            OutputFormat::Json | OutputFormat::Jsonl => {
                println!(
                    "{}",
                    serde_json::json!({
                        "phase": "dedup",
                        "pairs": pairs.iter().map(|p| serde_json::json!({
                            "a": p.a,
                            "b": p.b,
                            "similarity": p.similarity,
                        })).collect::<Vec<_>>(),
                    })
                );
            }
            _ => {
                let threshold_label = review_threshold
                    .map_or_else(|| "default".to_string(), |t| format!("{t:.2}"));
                if pairs.is_empty() {
                    println!(
                        "dedup    : no duplicate candidates above threshold {threshold_label}"
                    );
                } else {
                    println!(
                        "dedup    : {n} candidate pair(s) above threshold {threshold_label}",
                        n = pairs.len(),
                    );
                    for p in &pairs {
                        println!("  {:.3}  {}  {}", p.similarity, p.a, p.b);
                    }
                }
            }
        }
    }

    if run_decay {
        let outcome = ipc::entity_decay_relations(
            runtime,
            rt,
            decay_factor,
            decay_floor,
            dry_run,
        )
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

    Ok(())
}

/// `nexus graph entity duplicates` — Jaccard-similar entity pairs.
pub fn entity_duplicates(app: &mut App, threshold: f32) -> Result<()> {
    let format = app.format();
    let (runtime, rt) = app.runtime()?;
    let pairs = ipc::entity_find_duplicates(runtime, rt, Some(threshold))
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
                .map(|p| {
                    vec![
                        format!("{:.3}", p.similarity),
                        p.a.clone(),
                        p.b.clone(),
                    ]
                })
                .collect();
            print_list(format, headers, &rows);
        }
    }
    Ok(())
}
