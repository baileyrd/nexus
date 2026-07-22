//! BL-128 thin slice — personal entity index.
//!
//! Entities live as plain markdown files under `<forge>/entities/`. Each
//! file's YAML frontmatter carries the typed metadata; the body is a
//! free-form human description. This module parses every `*.md` under
//! that directory into an in-memory [`EntityIndex`] suitable for
//! synchronous lookups from the IPC dispatch.
//!
//! The thin-slice schema is deliberately narrow:
//!
//! ```yaml
//! ---
//! entity_type: person   # or "project" — only two recognised today
//! aliases: [DB, "Dr. Bailey"]
//! description: Engineer working on Nexus.
//! relations:
//!   - target: nexus
//!     type: works_on
//!     confidence: 0.9
//! ---
//! Free-form notes about the entity.
//! ```
//!
//! Anything outside this shape is preserved as best-effort:
//!   * Unknown `entity_type` values pass through verbatim — the agent
//!     prompt-prepend pass treats them as plain strings.
//!   * Missing `description` falls back to the first non-empty body
//!     paragraph (trimmed to 240 chars).
//!   * Missing `aliases` defaults to an empty list.
//!   * Missing `relations` defaults to an empty list.
//!   * `confidence` defaults to `1.0` when absent.
//!
//! Index lifecycle: the dispatch path rebuilds the index per IPC call
//! today (intentional — for a thin slice with O(dozens) of entity
//! files the parse cost is sub-millisecond). A future pass can layer a
//! cache invalidated by the existing file watcher; the shape of the
//! public API stays the same.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Directory under the forge root that holds entity files. Missing
/// directory is **not** an error — `load` returns an empty index so
/// forges that never use the feature pay no cost beyond a single
/// `try_exists`.
pub const ENTITIES_DIR: &str = "entities";

/// One parsed entity. Mirrors what lives in the markdown file plus a
/// derived `id` (the file stem) and `relpath` (`entities/<stem>.md`).
///
/// The `relations` list is the **outgoing** edges declared on this
/// entity. Incoming edges are reconstructed by scanning every entity
/// at query time — cheap for the thin slice's expected entity counts
/// (~dozens) and avoids a denormalised second index.
///
/// This is an **internal** Rust type — the IPC wire shape lives in
/// [`crate::ipc::EntityRecordRow`]. Keeping the projection layered
/// means a future change to the on-disk schema (e.g. adding an
/// embedding field) doesn't immediately bump the public TS contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EntityRecord {
    /// Canonical identifier — the file stem (e.g. `david-bailey` for
    /// `entities/david-bailey.md`).
    pub id: String,
    /// User-declared type. Free-form; the thin slice recognises
    /// `person` and `project` semantically but any value passes through.
    pub entity_type: String,
    /// Alternative names that resolve to this entity in `get` and
    /// score-boosted in `search`.
    pub aliases: Vec<String>,
    /// One-line description from frontmatter `description:` or, when
    /// absent, the first non-empty body paragraph (trimmed).
    pub description: String,
    /// Outgoing relations declared on this entity.
    pub relations: Vec<EntityRelation>,
    /// Forge-relative path of the source markdown file.
    pub relpath: String,
}

/// One outgoing relation pointing from an entity to another entity id.
/// Internal Rust type — wire shape lives in [`crate::ipc::EntityRelationRow`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EntityRelation {
    /// Target entity id (or alias — resolved at query time).
    pub target: String,
    /// Free-form relation kind. The thin slice does not normalise.
    #[serde(rename = "type")]
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`. Defaults to `1.0` when absent.
    pub confidence: f32,
}

/// Direction filter for [`EntityIndex::relations`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationDirection {
    /// Edges declared on the queried entity itself.
    Outgoing,
    /// Edges declared on other entities that target this one.
    Incoming,
    /// Both `Outgoing` and `Incoming` (the default).
    Both,
}

impl RelationDirection {
    /// Parse `"outgoing"` / `"incoming"` / `"both"` (case-insensitive).
    /// Unknown / empty values map to [`RelationDirection::Both`].
    #[must_use]
    pub fn parse(s: Option<&str>) -> Self {
        match s.unwrap_or("both").trim().to_ascii_lowercase().as_str() {
            "outgoing" | "out" => Self::Outgoing,
            "incoming" | "in" => Self::Incoming,
            _ => Self::Both,
        }
    }
}

/// One row in [`EntityIndex::relations`] result. Aliased targets are
/// resolved to their canonical id before this row is constructed.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRelation {
    /// Source entity id (always canonical).
    pub from: String,
    /// Target entity id (canonical when the alias resolves, else the
    /// raw on-disk target string).
    pub to: String,
    /// Free-form relation kind from the source entity's frontmatter.
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f32,
}

/// In-memory entity index. Built by [`EntityIndex::load`]; queries are
/// pure functions over its maps.
#[derive(Debug, Default)]
pub struct EntityIndex {
    /// Entities keyed by canonical id.
    by_id: BTreeMap<String, EntityRecord>,
    /// Alias → canonical id. Aliases that collide with a canonical id
    /// or another alias are dropped silently (the first writer wins).
    by_alias: BTreeMap<String, String>,
}

impl EntityIndex {
    /// Walk `<forge_root>/entities/` and build the index. Files that
    /// fail to parse (bad YAML, missing `entity_type`) are skipped
    /// with a `tracing::warn` — one broken entity must not poison the
    /// whole index.
    ///
    /// A missing `entities/` directory is not an error; the resulting
    /// index is empty.
    #[must_use]
    pub fn load(forge_root: &Path) -> Self {
        let dir = forge_root.join(ENTITIES_DIR);
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            return Self::default();
        };
        let mut index = Self::default();
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !is_entity_file(&path) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let relpath = format!(
                "{ENTITIES_DIR}/{}",
                path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
            );
            match parse_entity(stem, &relpath, &content) {
                Ok(record) => index.insert(record),
                Err(reason) => {
                    tracing::warn!(
                        target: "nexus_storage::entity_index",
                        path = %path.display(),
                        reason = %reason,
                        "skipped entity file"
                    );
                }
            }
        }
        index
    }

    fn insert(&mut self, record: EntityRecord) {
        // Aliases register before the canonical id so an alias that
        // happens to equal a different entity's id can't overwrite it.
        for alias in &record.aliases {
            let normalised = alias.trim().to_string();
            if normalised.is_empty() || normalised == record.id {
                continue;
            }
            self.by_alias
                .entry(normalised)
                .or_insert_with(|| record.id.clone());
        }
        self.by_id.insert(record.id.clone(), record);
    }

    /// Number of indexed entities.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// `true` when no entities are indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Look up by canonical id, then by alias. Returns `None` when
    /// neither resolves.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&EntityRecord> {
        let key = key.trim();
        if let Some(record) = self.by_id.get(key) {
            return Some(record);
        }
        let canonical = self.by_alias.get(key)?;
        self.by_id.get(canonical)
    }

    /// Substring search over `id` / `aliases` / `description`. Returns
    /// up to `limit` hits ordered by descending score then ascending
    /// id. An empty query yields the lexicographically-first `limit`
    /// records (useful for the agent prepend path's "no specific
    /// match — give me the first few" fallback).
    ///
    /// Scoring (intentionally simple — a real ranker would be a
    /// future BL-128 expansion):
    /// * exact id match: 100
    /// * exact alias match: 90
    /// * id contains query: 75
    /// * alias contains query: 60
    /// * description contains query: 40
    ///
    /// `entity_type_filter`, when `Some`, restricts hits to entities
    /// whose `entity_type` matches (case-insensitive).
    #[must_use]
    pub fn search(
        &self,
        query: &str,
        entity_type_filter: Option<&str>,
        limit: usize,
    ) -> Vec<EntitySearchHit> {
        let query = query.trim().to_ascii_lowercase();
        let type_filter = entity_type_filter.map(|t| t.trim().to_ascii_lowercase());
        let mut scored: Vec<(i32, &EntityRecord)> = self
            .by_id
            .values()
            .filter(|r| {
                type_filter
                    .as_deref()
                    .is_none_or(|t| r.entity_type.eq_ignore_ascii_case(t))
            })
            .filter_map(|record| {
                let score = if query.is_empty() {
                    1
                } else {
                    score_record(record, &query)
                };
                (score > 0).then_some((score, record))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        scored
            .into_iter()
            .take(limit)
            .map(|(score, record)| EntitySearchHit {
                id: record.id.clone(),
                entity_type: record.entity_type.clone(),
                description: record.description.clone(),
                relpath: record.relpath.clone(),
                score,
            })
            .collect()
    }

    /// Outgoing / incoming / both relations for `id_or_alias`.
    /// Outgoing rows are sourced directly from the record's
    /// `relations` field; incoming rows are reconstructed by scanning
    /// every other entity's outgoing edges. Order: outgoing first,
    /// then incoming, stable by (from, to, kind).
    ///
    /// Returns an empty Vec when the id doesn't resolve.
    #[must_use]
    pub fn relations(
        &self,
        id_or_alias: &str,
        direction: RelationDirection,
    ) -> Vec<ResolvedRelation> {
        let Some(record) = self.get(id_or_alias) else {
            return Vec::new();
        };
        let canonical = &record.id;
        let mut out: Vec<ResolvedRelation> = Vec::new();
        if matches!(
            direction,
            RelationDirection::Outgoing | RelationDirection::Both
        ) {
            for rel in &record.relations {
                let resolved_target = self
                    .by_alias
                    .get(&rel.target)
                    .cloned()
                    .unwrap_or_else(|| rel.target.clone());
                out.push(ResolvedRelation {
                    from: canonical.clone(),
                    to: resolved_target,
                    kind: rel.kind.clone(),
                    confidence: rel.confidence,
                });
            }
        }
        if matches!(
            direction,
            RelationDirection::Incoming | RelationDirection::Both
        ) {
            for other in self.by_id.values() {
                if &other.id == canonical {
                    continue;
                }
                for rel in &other.relations {
                    let resolved_target = self
                        .by_alias
                        .get(&rel.target)
                        .cloned()
                        .unwrap_or_else(|| rel.target.clone());
                    if &resolved_target == canonical {
                        out.push(ResolvedRelation {
                            from: other.id.clone(),
                            to: canonical.clone(),
                            kind: rel.kind.clone(),
                            confidence: rel.confidence,
                        });
                    }
                }
            }
        }
        out.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then_with(|| a.to.cmp(&b.to))
                .then_with(|| a.kind.cmp(&b.kind))
        });
        out
    }

    /// Find pairs of entities whose normalised token sets overlap by
    /// at least `threshold` (Jaccard similarity over `id + aliases +
    /// description`). Pairs are returned sorted by descending
    /// similarity then ascending `(a, b)` for determinism.
    ///
    /// BL-129's Dream-Cycle dedup phase reads this list: pairs above
    /// the auto-merge threshold (default 0.97) are merged silently,
    /// the rest queued for user review.
    ///
    /// Different `entity_type`s never pair — a `person` named "java"
    /// and a `skill` named "java" are not duplicates of each other.
    #[must_use]
    pub fn find_duplicates(&self, threshold: f32) -> Vec<DuplicateCandidate> {
        let entries: Vec<(&String, &EntityRecord, std::collections::HashSet<String>)> = self
            .by_id
            .values()
            .map(|r| {
                let mut tokens = tokenise(&r.id);
                for alias in &r.aliases {
                    tokens.extend(tokenise(alias));
                }
                tokens.extend(tokenise(&r.description));
                (&r.id, r, tokens)
            })
            .collect();

        let mut candidates = Vec::new();
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                let (id_a, rec_a, tok_a) = &entries[i];
                let (id_b, rec_b, tok_b) = &entries[j];
                if !rec_a.entity_type.eq_ignore_ascii_case(&rec_b.entity_type) {
                    continue;
                }
                let sim = jaccard(tok_a, tok_b);
                if sim >= threshold {
                    let (a, b) = if id_a <= id_b {
                        ((*id_a).clone(), (*id_b).clone())
                    } else {
                        ((*id_b).clone(), (*id_a).clone())
                    };
                    candidates.push(DuplicateCandidate {
                        a,
                        b,
                        similarity: sim,
                    });
                }
            }
        }
        candidates.sort_by(|x, y| {
            y.similarity
                .partial_cmp(&x.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| x.a.cmp(&y.a))
                .then_with(|| x.b.cmp(&y.b))
        });
        candidates
    }

    /// Collect every outgoing relation across the index whose
    /// `confidence` is at or below `threshold`, then take at most
    /// `limit` rows ordered by ascending confidence then ascending
    /// `(from, target, kind)`.
    ///
    /// Drives the BL-129 Dream-Cycle inbox: the LLM
    /// `infer_entity_relations` handler writes proposals at
    /// `confidence: 0.5`, so the inbox passes that as the threshold to
    /// surface exactly the unconfirmed drafts.
    ///
    /// Returns `(rows, total)` where `total` counts every qualifying
    /// relation across the index regardless of `limit`, so callers can
    /// surface a "showing N of M" hint.
    #[must_use]
    pub fn list_draft_relations(
        &self,
        threshold: f32,
        limit: usize,
    ) -> (Vec<DraftRelationCandidate>, u32) {
        let threshold = threshold.clamp(0.0, 1.0);
        let mut all: Vec<DraftRelationCandidate> = Vec::new();
        for record in self.by_id.values() {
            for rel in &record.relations {
                if rel.confidence <= threshold + f32::EPSILON {
                    all.push(DraftRelationCandidate {
                        from: record.id.clone(),
                        target: rel.target.clone(),
                        kind: rel.kind.clone(),
                        confidence: rel.confidence,
                        relpath: record.relpath.clone(),
                    });
                }
            }
        }
        all.sort_by(|x, y| {
            x.confidence
                .partial_cmp(&y.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| x.from.cmp(&y.from))
                .then_with(|| x.target.cmp(&y.target))
                .then_with(|| x.kind.cmp(&y.kind))
        });
        let total = u32::try_from(all.len()).unwrap_or(u32::MAX);
        all.truncate(limit);
        (all, total)
    }
}

/// One row returned by [`EntityIndex::list_draft_relations`]. Wire
/// shape lives in [`crate::ipc::DraftRelationRow`]; this struct is the
/// internal projection so the IPC dispatch is the only place that
/// maps between them.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftRelationCandidate {
    /// Canonical id of the source entity that declares the relation.
    pub from: String,
    /// Target as it appears in the source file (may be an alias).
    pub target: String,
    /// Relation kind (canonical — write-time normalisation has
    /// already happened by the time the index sees it).
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f32,
    /// Forge-relative path of the source entity's markdown file.
    pub relpath: String,
}

/// One pair returned by [`EntityIndex::find_duplicates`].
#[derive(Debug, Clone, PartialEq)]
pub struct DuplicateCandidate {
    /// Lexicographically-smaller entity id.
    pub a: String,
    /// Lexicographically-greater entity id.
    pub b: String,
    /// Jaccard token similarity in `[0.0, 1.0]`.
    pub similarity: f32,
}

fn tokenise(text: &str) -> std::collections::HashSet<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 1)
        .map(str::to_string)
        .collect()
}

fn jaccard(a: &std::collections::HashSet<String>, b: &std::collections::HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersect = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let val = intersect as f32 / union as f32;
        val
    }
}

// ── Canonical vocabularies (BL-128 close) ────────────────────────────────────

/// The 11 canonical entity types from the Thoth vocabulary. The
/// thin-slice [`parse_entity`] accepts any string; this list is the
/// vocabulary that downstream consumers (CLI guidance, validation,
/// the deferred shell explorer) recognise.
pub const ENTITY_TYPES: &[&str] = &[
    "person",
    "preference",
    "fact",
    "event",
    "place",
    "project",
    "organisation",
    "concept",
    "skill",
    "media",
    "self_knowledge",
];

/// The canonical relation vocabulary — 40+ entries grouped by family,
/// location, work, knowledge, media, ownership, temporal, causality,
/// and generic semantics. LLM-emitted variants flow through
/// [`normalize_relation_type`] before any of these match.
pub const RELATION_TYPES: &[&str] = &[
    // family / social
    "knows",
    "friend_of",
    "married_to",
    "parent_of",
    "child_of",
    "sibling_of",
    "partner_of",
    // location
    "lives_in",
    "works_at",
    "born_in",
    "located_in",
    "from",
    // work
    "works_on",
    "manages",
    "managed_by",
    "employed_by",
    "employs",
    "collaborates_with",
    // knowledge
    "proficient_in",
    "certified_in",
    "studies",
    "studied_at",
    "knows_about",
    // media
    "reading",
    "watching",
    "listening_to",
    "authored",
    "created",
    // ownership / membership
    "owns",
    "member_of",
    "belongs_to",
    "part_of",
    // temporal
    "preceded_by",
    "followed_by",
    "happened_on",
    // causality
    "causes",
    "caused_by",
    "enables",
    // generic
    "related_to",
    "references",
    "mentioned_in",
];

/// Normalise an LLM-or-user-provided relation type to one of the
/// canonical [`RELATION_TYPES`]. Lowercase + replace ` ` / `-` with
/// `_`, then look up a small alias table. Unknown inputs fall back
/// to `"related_to"` — same choice Thoth makes, and a safer landing
/// zone for AI-proposed relations than erroring out.
#[must_use]
pub fn normalize_relation_type(input: &str) -> &'static str {
    let lower = input.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let alias: &str = match lower.as_str() {
        "is_friend_of" | "friend" => "friend_of",
        "spouse" | "spouse_of" | "wife_of" | "husband_of" => "married_to",
        "is_parent_of" | "father_of" | "mother_of" => "parent_of",
        "is_child_of" | "son_of" | "daughter_of" => "child_of",
        "brother_of" | "sister_of" => "sibling_of",
        "lives_at" | "resides_in" | "resides_at" => "lives_in",
        "born_at" => "born_in",
        "located_at" | "in" => "located_in",
        "from_city" | "originally_from" => "from",
        "working_on" | "contributes_to" => "works_on",
        "is_manager_of" | "manages_team" => "manages",
        "is_managed_by" | "reports_to" => "managed_by",
        "works_for" | "employee_of" => "employed_by",
        "hires" => "employs",
        "works_with" => "collaborates_with",
        "skilled_in" | "expert_in" | "fluent_in" => "proficient_in",
        "has_certification_in" => "certified_in",
        "studying" | "learning" => "studies",
        "alumnus_of" | "graduated_from" => "studied_at",
        "currently_reading" => "reading",
        "currently_watching" => "watching",
        "currently_listening_to" | "listening" => "listening_to",
        "wrote" | "author_of" | "authored_by" => "authored",
        "created_by" | "made" => "created",
        "owner_of" | "has" => "owns",
        "is_member_of" => "member_of",
        "before" | "comes_before" => "preceded_by",
        "after" | "comes_after" | "next" => "followed_by",
        "on" | "occurred_on" | "happened_at" => "happened_on",
        "leads_to" | "results_in" => "causes",
        "due_to" | "result_of" => "caused_by",
        other => other,
    };
    for canon in RELATION_TYPES {
        if alias == *canon {
            return canon;
        }
    }
    "related_to"
}

// ── Upsert (file-as-truth write-through) ─────────────────────────────────────

/// Frontmatter payload for [`upsert_entity_file`]. The `id` becomes
/// the file stem (`entities/<id>.md`); the rest of the fields map
/// directly to the on-disk YAML keys recognised by [`parse_entity`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityUpsert {
    /// Canonical id — becomes the markdown file stem.
    pub id: String,
    /// `entity_type:` frontmatter key.
    pub entity_type: String,
    /// `aliases:` frontmatter key. Empty list omits the field on disk.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// `description:` frontmatter key. Empty string omits the field.
    #[serde(default)]
    pub description: String,
    /// `relations:` frontmatter list. Empty list omits the field.
    /// Relation kinds pass through [`normalize_relation_type`] so the
    /// on-disk file always carries canonical vocabulary regardless of
    /// what the caller submitted.
    #[serde(default)]
    pub relations: Vec<EntityUpsertRelation>,
}

/// One relation entry inside [`EntityUpsert::relations`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityUpsertRelation {
    /// Target entity id (or alias — preserved verbatim on disk).
    pub target: String,
    /// Free-form relation kind. Normalised before writing.
    #[serde(rename = "type")]
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`. Defaults to `1.0` when absent.
    #[serde(default)]
    pub confidence: Option<f32>,
}

/// Render an [`EntityUpsert`] into the canonical on-disk markdown
/// representation. Pure function — extracted so the upsert IPC
/// handler is a thin wrapper around `atomic_write`, and so unit
/// tests can pin the wire-shape projection without touching the
/// filesystem.
#[must_use]
pub fn render_entity_markdown(payload: &EntityUpsert) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("---\n");
    let _ = writeln!(out, "entity_type: {}", yaml_escape(&payload.entity_type));
    if !payload.aliases.is_empty() {
        out.push_str("aliases:\n");
        for alias in &payload.aliases {
            let _ = writeln!(out, "  - {}", yaml_escape(alias));
        }
    }
    if !payload.description.is_empty() {
        let _ = writeln!(out, "description: {}", yaml_escape(&payload.description));
    }
    if !payload.relations.is_empty() {
        out.push_str("relations:\n");
        for rel in &payload.relations {
            let _ = writeln!(out, "  - target: {}", yaml_escape(&rel.target));
            let canon = normalize_relation_type(&rel.kind);
            let _ = writeln!(out, "    type: {canon}");
            if let Some(conf) = rel.confidence {
                let _ = writeln!(out, "    confidence: {}", conf.clamp(0.0, 1.0));
            }
        }
    }
    out.push_str("---\n");
    out
}

fn yaml_escape(s: &str) -> String {
    let needs_quotes = s.is_empty()
        || s.starts_with([
            ' ', '\t', '"', '\'', '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|',
            '>', '%', '@', '`',
        ])
        || s.contains(['\n', '"', ':'])
        || s.trim() != s;
    if needs_quotes {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

/// One hit in [`EntityIndex::search`].
#[derive(Debug, Clone, PartialEq)]
pub struct EntitySearchHit {
    /// Canonical entity id.
    pub id: String,
    /// `entity_type` from frontmatter.
    pub entity_type: String,
    /// One-line description (frontmatter or first body paragraph).
    pub description: String,
    /// Forge-relative path of the source markdown file.
    pub relpath: String,
    /// Match score per [`EntityIndex::search`] doc-comment.
    pub score: i32,
}

fn score_record(record: &EntityRecord, query_lc: &str) -> i32 {
    let id_lc = record.id.to_ascii_lowercase();
    if id_lc == query_lc {
        return 100;
    }
    for alias in &record.aliases {
        if alias.eq_ignore_ascii_case(query_lc) {
            return 90;
        }
    }
    if id_lc.contains(query_lc) {
        return 75;
    }
    for alias in &record.aliases {
        if alias.to_ascii_lowercase().contains(query_lc) {
            return 60;
        }
    }
    if record.description.to_ascii_lowercase().contains(query_lc) {
        return 40;
    }
    0
}

fn is_entity_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

/// Parse one entity file's source into an [`EntityRecord`]. Exposed
/// for unit tests; production callers go through [`EntityIndex::load`].
///
/// `stem` is the file stem (becomes the canonical id). `relpath` is
/// the forge-relative path (used by the search hit + get response).
///
/// # Errors
///
/// Returns a human-readable string when the YAML frontmatter is
/// missing, fails to decode, or omits the mandatory `entity_type`
/// key. The `EntityIndex::load` caller swallows the error into a
/// `tracing::warn` so one broken file doesn't poison the index.
pub fn parse_entity(stem: &str, relpath: &str, content: &str) -> Result<EntityRecord, String> {
    let (yaml_src, body) = split_frontmatter(content);
    let yaml_src = yaml_src.ok_or_else(|| "no YAML frontmatter".to_string())?;
    let raw: RawEntityFrontmatter =
        serde_norway::from_str(yaml_src).map_err(|e| format!("frontmatter YAML decode: {e}"))?;
    let entity_type = raw
        .entity_type
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing 'entity_type' frontmatter key".to_string())?;
    let aliases = raw
        .aliases
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let description = raw
        .description
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| description_from_body(body));
    let relations = raw
        .relations
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| {
            let target = r.target.trim().to_string();
            let kind = r.kind.trim().to_string();
            if target.is_empty() || kind.is_empty() {
                return None;
            }
            let confidence = r.confidence.unwrap_or(1.0).clamp(0.0, 1.0);
            Some(EntityRelation {
                target,
                kind,
                confidence,
            })
        })
        .collect();
    Ok(EntityRecord {
        id: stem.to_string(),
        entity_type,
        aliases,
        description,
        relations,
        relpath: relpath.to_string(),
    })
}

#[derive(Debug, Deserialize)]
struct RawEntityFrontmatter {
    entity_type: Option<String>,
    #[serde(default)]
    aliases: Option<Vec<String>>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    relations: Option<Vec<RawRelation>>,
}

#[derive(Debug, Deserialize)]
struct RawRelation {
    target: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    confidence: Option<f32>,
}

/// Split a markdown source into `(yaml_frontmatter_src, body)`. Returns
/// `(None, content)` when the file has no leading `---` fence.
fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let after_open = if let Some(s) = content.strip_prefix("---\r\n") {
        s
    } else if let Some(s) = content.strip_prefix("---\n") {
        s
    } else {
        return (None, content);
    };
    let close_pattern = "\n---";
    let Some(close_pos) = after_open.find(close_pattern) else {
        return (None, content);
    };
    let yaml = &after_open[..close_pos];
    let after_close = &after_open[close_pos + close_pattern.len()..];
    let body = after_close
        .trim_start_matches('\r')
        .trim_start_matches('\n');
    (Some(yaml), body)
}

const DESCRIPTION_FALLBACK_CAP: usize = 240;

fn description_from_body(body: &str) -> String {
    let first = body
        .split("\n\n")
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("");
    truncate_at_char_boundary(first, DESCRIPTION_FALLBACK_CAP)
}

fn truncate_at_char_boundary(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i == max_chars {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

// ── BL-129 Dream Cycle — merge ──────────────────────────────────────────────

/// Result of [`merge_records`]. The `payload` is ready to be passed
/// straight to [`render_entity_markdown`].
#[derive(Debug, Clone)]
pub struct MergedEntity {
    /// The combined entity, ready to render.
    pub payload: EntityUpsert,
    /// New aliases on the survivor (includes the dropped entity's id
    /// when it wasn't already aliased).
    pub aliases_added: u32,
    /// Relations added to the survivor (deduplicated on `(target, kind)`).
    pub relations_added: u32,
}

/// Merge `drop` into `keep`. Pure function — caller owns the I/O.
/// Aliases and relations are unioned (deduplicated case-insensitively
/// on alias text, on `(target, kind)` for relations with the maximum
/// confidence winning on conflict). The longer description survives;
/// ties prefer `keep`. The dropped entity's canonical id is added to
/// the survivor's alias list so back-references from other entities
/// still resolve.
#[must_use]
pub fn merge_records(keep: &EntityRecord, drop: &EntityRecord) -> MergedEntity {
    use std::collections::BTreeMap;

    // Aliases: start with keep's, then add drop's aliases that aren't
    // already present (case-insensitive). Always add drop's canonical id.
    let mut alias_keys: std::collections::BTreeSet<String> = keep
        .aliases
        .iter()
        .map(|a| a.to_ascii_lowercase())
        .collect();
    let mut aliases: Vec<String> = keep.aliases.clone();
    let mut aliases_added = 0u32;
    let try_add_alias = |a: &str,
                         alias_keys: &mut std::collections::BTreeSet<String>,
                         aliases: &mut Vec<String>,
                         added: &mut u32| {
        let trimmed = a.trim();
        if trimmed.is_empty() || trimmed == keep.id {
            return;
        }
        let key = trimmed.to_ascii_lowercase();
        if alias_keys.insert(key) {
            aliases.push(trimmed.to_string());
            *added += 1;
        }
    };
    for a in &drop.aliases {
        try_add_alias(a, &mut alias_keys, &mut aliases, &mut aliases_added);
    }
    // Add drop's canonical id as an alias so dangling references resolve.
    try_add_alias(&drop.id, &mut alias_keys, &mut aliases, &mut aliases_added);

    // Description: longer wins; tie favours `keep`.
    let description = if drop.description.chars().count() > keep.description.chars().count() {
        drop.description.clone()
    } else {
        keep.description.clone()
    };

    // Relations: keep's first, then merge drop's. Dedup on (target, kind);
    // on conflict keep the max confidence. Order: insertion (keep first).
    let mut order: Vec<(String, String)> = Vec::new();
    let mut by_key: BTreeMap<(String, String), f32> = BTreeMap::new();
    let mut relations_added = 0u32;
    for r in &keep.relations {
        let k = (r.target.clone(), r.kind.clone());
        if by_key.insert(k.clone(), r.confidence).is_none() {
            order.push(k);
        }
    }
    for r in &drop.relations {
        let k = (r.target.clone(), r.kind.clone());
        match by_key.get_mut(&k) {
            Some(existing) => {
                if r.confidence > *existing {
                    *existing = r.confidence;
                }
            }
            None => {
                by_key.insert(k.clone(), r.confidence);
                order.push(k);
                relations_added += 1;
            }
        }
    }
    let relations: Vec<EntityUpsertRelation> = order
        .into_iter()
        .map(|(target, kind)| {
            let confidence = by_key
                .get(&(target.clone(), kind.clone()))
                .copied()
                .unwrap_or(1.0);
            EntityUpsertRelation {
                target,
                kind,
                confidence: Some(confidence),
            }
        })
        .collect();

    let payload = EntityUpsert {
        id: keep.id.clone(),
        entity_type: keep.entity_type.clone(),
        aliases,
        description,
        relations,
    };
    MergedEntity {
        payload,
        aliases_added,
        relations_added,
    }
}

// ── BL-129 Dream Cycle — confidence decay ───────────────────────────────────

/// Parameters for [`decay_file_content`]. Defaults match the
/// Dream-Cycle config block (`factor = 0.95`, `floor = 0.10`).
#[derive(Debug, Clone, Copy)]
pub struct DecayParams {
    /// Multiplicative factor applied to each relation's confidence.
    /// Clamped to `[0.0, 1.0]`; values at or above `1.0` short-circuit
    /// the decay (`decay_file_content` returns `None`).
    pub factor: f32,
    /// Lower bound for confidence post-decay. Relations already at or
    /// below the floor are left alone so repeated cycles converge.
    pub floor: f32,
}

impl Default for DecayParams {
    fn default() -> Self {
        Self {
            factor: 0.95,
            floor: 0.10,
        }
    }
}

/// Outcome of [`decay_file_content`] for one entity file. `None`
/// means the file is unchanged (no parsable frontmatter, no
/// relations, every relation already at floor, or `factor` is a no-op).
#[derive(Debug, Clone, PartialEq)]
pub struct DecayedFile {
    /// New full file content, ready for atomic write.
    pub content: String,
    /// Number of relations whose confidence was reduced this pass.
    pub relations_decayed: u32,
    /// Number of relations that landed exactly on `floor` as a result
    /// of this pass. Pre-existing at-floor relations are skipped and
    /// do not count.
    pub relations_at_floor: u32,
}

/// Apply confidence decay to every relation in one entity markdown
/// source string. Pure transform — the caller owns the I/O.
///
/// Frontmatter keys outside the thin-slice schema are dropped on
/// rewrite (same posture as `entity_upsert`); the free-form markdown
/// body is preserved verbatim. Relation kinds re-flow through
/// [`normalize_relation_type`] inside [`render_entity_markdown`] so
/// any drift toward non-canonical vocabulary is corrected on the way
/// out.
#[must_use]
pub fn decay_file_content(content: &str, params: &DecayParams) -> Option<DecayedFile> {
    let factor = params.factor.clamp(0.0, 1.0);
    let floor = params.floor.clamp(0.0, 1.0);
    // A factor at-or-above 1 cannot lower any confidence — short-circuit
    // before touching the parser.
    if factor >= 1.0 - f32::EPSILON {
        return None;
    }
    let (yaml_src, body) = split_frontmatter(content);
    let yaml_src = yaml_src?;
    let raw: RawEntityFrontmatter = serde_norway::from_str(yaml_src).ok()?;
    let entity_type = raw
        .entity_type
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    let relations_in = raw.relations.unwrap_or_default();
    if relations_in.is_empty() {
        return None;
    }

    let mut relations_decayed = 0u32;
    let mut relations_at_floor = 0u32;
    let mut any_changed = false;
    let mut out: Vec<EntityUpsertRelation> = Vec::with_capacity(relations_in.len());
    for r in relations_in {
        let target = r.target.trim().to_string();
        let kind = r.kind.trim().to_string();
        if target.is_empty() || kind.is_empty() {
            continue;
        }
        let old = r.confidence.unwrap_or(1.0).clamp(0.0, 1.0);
        let new = if old <= floor + f32::EPSILON {
            // Already at-or-below floor — preserve verbatim so the
            // round-trip is byte-stable for these relations.
            old
        } else {
            let decayed = (old * factor).max(floor);
            if decayed < old - f32::EPSILON {
                relations_decayed += 1;
                any_changed = true;
                if (decayed - floor).abs() <= f32::EPSILON {
                    relations_at_floor += 1;
                }
            }
            decayed
        };
        out.push(EntityUpsertRelation {
            target,
            kind,
            confidence: Some(new),
        });
    }
    if !any_changed {
        return None;
    }

    let aliases = raw
        .aliases
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let description = raw
        .description
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    let payload = EntityUpsert {
        id: String::new(), // not consumed by render_entity_markdown
        entity_type,
        aliases,
        description,
        relations: out,
    };
    let mut new_content = render_entity_markdown(&payload);
    if !body.is_empty() {
        // `split_frontmatter` strips leading newlines; re-insert one
        // blank-line separator so the file's body stays readable.
        new_content.push('\n');
        new_content.push_str(body);
    }
    Some(DecayedFile {
        content: new_content,
        relations_decayed,
        relations_at_floor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("tempdir")
    }

    fn write_entity(forge: &Path, stem: &str, frontmatter: &str, body: &str) {
        let dir = forge.join(ENTITIES_DIR);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{stem}.md"));
        fs::write(&path, format!("---\n{frontmatter}---\n{body}")).unwrap();
    }

    #[test]
    fn missing_entities_dir_yields_empty_index() {
        let dir = tempdir();
        let index = EntityIndex::load(dir.path());
        assert!(index.is_empty());
    }

    #[test]
    fn parses_minimal_entity() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\ndescription: A friend.\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let rec = index.get("alice").expect("alice resolves");
        assert_eq!(rec.entity_type, "person");
        assert_eq!(rec.description, "A friend.");
        assert!(rec.aliases.is_empty());
        assert!(rec.relations.is_empty());
        assert_eq!(rec.relpath, "entities/alice.md");
    }

    #[test]
    fn alias_resolves_to_canonical() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\naliases: [Al, \"Dr. Smith\"]\n",
            "Body.",
        );
        let index = EntityIndex::load(dir.path());
        assert_eq!(index.get("Al").unwrap().id, "alice");
        assert_eq!(index.get("Dr. Smith").unwrap().id, "alice");
        assert!(index.get("nope").is_none());
    }

    #[test]
    fn description_falls_back_to_first_paragraph() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "nexus",
            "entity_type: project\n",
            "\n\n  First non-empty paragraph.  \n\nSecond.",
        );
        let index = EntityIndex::load(dir.path());
        let rec = index.get("nexus").unwrap();
        assert_eq!(rec.description, "First non-empty paragraph.");
    }

    #[test]
    fn description_truncated_at_240_chars() {
        let dir = tempdir();
        let body: String = "x".repeat(300);
        write_entity(dir.path(), "long", "entity_type: project\n", &body);
        let index = EntityIndex::load(dir.path());
        let rec = index.get("long").unwrap();
        assert_eq!(rec.description.chars().count(), 241); // 240 chars + ellipsis
        assert!(rec.description.ends_with('…'));
    }

    #[test]
    fn missing_entity_type_is_skipped() {
        let dir = tempdir();
        write_entity(dir.path(), "broken", "description: hi\n", "");
        let index = EntityIndex::load(dir.path());
        assert!(index.is_empty());
    }

    #[test]
    fn malformed_yaml_is_skipped() {
        let dir = tempdir();
        write_entity(dir.path(), "broken", "entity_type: : : :\n", "");
        let index = EntityIndex::load(dir.path());
        assert!(index.is_empty());
    }

    #[test]
    fn relations_default_confidence_and_clamp() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\nrelations:\n  - target: nexus\n    type: works_on\n  - target: bob\n    type: knows\n    confidence: 2.0\n  - target: cara\n    type: knows\n    confidence: -0.5\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let rec = index.get("alice").unwrap();
        assert_eq!(rec.relations.len(), 3);
        assert!((rec.relations[0].confidence - 1.0).abs() < 1e-6);
        assert!((rec.relations[1].confidence - 1.0).abs() < 1e-6);
        assert!(rec.relations[2].confidence.abs() < 1e-6);
    }

    #[test]
    fn search_ranks_exact_id_over_substring() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\ndescription: lives in alice springs\n",
            "",
        );
        write_entity(
            dir.path(),
            "alice-springs",
            "entity_type: place\ndescription: A town.\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let hits = index.search("alice", None, 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, "alice");
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn search_filters_by_entity_type() {
        let dir = tempdir();
        write_entity(dir.path(), "alice", "entity_type: person\n", "");
        write_entity(dir.path(), "nexus", "entity_type: project\n", "");
        let index = EntityIndex::load(dir.path());
        let hits = index.search("", Some("person"), 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "alice");
    }

    #[test]
    fn search_empty_query_returns_lexicographic_order() {
        let dir = tempdir();
        write_entity(dir.path(), "charlie", "entity_type: person\n", "");
        write_entity(dir.path(), "alice", "entity_type: person\n", "");
        write_entity(dir.path(), "bob", "entity_type: person\n", "");
        let index = EntityIndex::load(dir.path());
        let hits = index.search("", None, 10);
        let ids: Vec<_> = hits.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, vec!["alice", "bob", "charlie"]);
    }

    #[test]
    fn relations_outgoing_and_incoming() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\nrelations:\n  - target: nexus\n    type: works_on\n",
            "",
        );
        write_entity(
            dir.path(),
            "bob",
            "entity_type: person\nrelations:\n  - target: alice\n    type: knows\n",
            "",
        );
        write_entity(dir.path(), "nexus", "entity_type: project\n", "");
        let index = EntityIndex::load(dir.path());

        let out = index.relations("alice", RelationDirection::Outgoing);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].to, "nexus");
        assert_eq!(out[0].kind, "works_on");

        let inc = index.relations("alice", RelationDirection::Incoming);
        assert_eq!(inc.len(), 1);
        assert_eq!(inc[0].from, "bob");

        let both = index.relations("alice", RelationDirection::Both);
        assert_eq!(both.len(), 2);
    }

    #[test]
    fn relations_resolve_alias_target() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\nrelations:\n  - target: \"Dr. Bailey\"\n    type: knows\n",
            "",
        );
        write_entity(
            dir.path(),
            "david-bailey",
            "entity_type: person\naliases: [\"Dr. Bailey\"]\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let out = index.relations("alice", RelationDirection::Outgoing);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].to, "david-bailey");

        let inc = index.relations("david-bailey", RelationDirection::Incoming);
        assert_eq!(inc.len(), 1);
        assert_eq!(inc[0].from, "alice");
    }

    #[test]
    fn relations_for_unknown_id_returns_empty() {
        let dir = tempdir();
        let index = EntityIndex::load(dir.path());
        assert!(index.relations("ghost", RelationDirection::Both).is_empty());
    }

    #[test]
    fn parse_direction_known_and_unknown() {
        assert_eq!(
            RelationDirection::parse(Some("outgoing")),
            RelationDirection::Outgoing
        );
        assert_eq!(
            RelationDirection::parse(Some("OUT")),
            RelationDirection::Outgoing
        );
        assert_eq!(
            RelationDirection::parse(Some("incoming")),
            RelationDirection::Incoming
        );
        assert_eq!(
            RelationDirection::parse(Some("both")),
            RelationDirection::Both
        );
        assert_eq!(RelationDirection::parse(None), RelationDirection::Both);
        assert_eq!(
            RelationDirection::parse(Some("nope")),
            RelationDirection::Both
        );
    }

    #[test]
    fn canonical_vocabularies_are_populated() {
        assert_eq!(ENTITY_TYPES.len(), 11);
        assert!(ENTITY_TYPES.contains(&"person"));
        assert!(ENTITY_TYPES.contains(&"self_knowledge"));
        assert!(
            RELATION_TYPES.len() >= 40,
            "BL-128 DoD requires ≥40 relations, got {}",
            RELATION_TYPES.len()
        );
        assert!(RELATION_TYPES.contains(&"related_to"));
    }

    #[test]
    fn normalize_relation_type_canonicalises_common_variants() {
        assert_eq!(normalize_relation_type("Spouse"), "married_to");
        assert_eq!(normalize_relation_type("works for"), "employed_by");
        assert_eq!(normalize_relation_type("REPORTS-TO"), "managed_by");
        assert_eq!(normalize_relation_type("knows"), "knows");
        assert_eq!(normalize_relation_type("zorgblatts"), "related_to");
    }

    #[test]
    fn find_duplicates_pairs_high_overlap_same_type() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\naliases: [Al]\ndescription: Forge engineer working on storage.\n",
            "",
        );
        write_entity(
            dir.path(),
            "al",
            "entity_type: person\naliases: [Alice]\ndescription: Storage engineer working on forge.\n",
            "",
        );
        write_entity(
            dir.path(),
            "nexus",
            "entity_type: project\ndescription: Forge engineer working on storage.\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let dupes = index.find_duplicates(0.5);
        assert!(
            dupes.iter().any(|d| d.a == "al" && d.b == "alice"),
            "expected alice/al pair, got {dupes:?}"
        );
        assert!(
            dupes.iter().all(|d| d.a != "nexus" && d.b != "nexus"),
            "different entity_types must not pair (got {dupes:?})"
        );
    }

    #[test]
    fn find_duplicates_orders_by_descending_similarity() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alpha",
            "entity_type: person\ndescription: red green blue\n",
            "",
        );
        write_entity(
            dir.path(),
            "beta",
            "entity_type: person\ndescription: red green blue yellow\n",
            "",
        );
        write_entity(
            dir.path(),
            "gamma",
            "entity_type: person\ndescription: red blue indigo violet\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let dupes = index.find_duplicates(0.1);
        assert!(dupes.len() >= 2);
        for window in dupes.windows(2) {
            assert!(
                window[0].similarity >= window[1].similarity,
                "duplicates must be sorted descending: {dupes:?}"
            );
        }
    }

    #[test]
    fn render_entity_markdown_round_trips_through_parser() {
        let payload = EntityUpsert {
            id: "alice".to_string(),
            entity_type: "person".to_string(),
            aliases: vec!["Al".to_string(), "Dr. Smith".to_string()],
            description: "Engineer on the storage team.".to_string(),
            relations: vec![
                EntityUpsertRelation {
                    target: "nexus".to_string(),
                    kind: "works on".to_string(),
                    confidence: Some(0.9),
                },
                EntityUpsertRelation {
                    target: "bob".to_string(),
                    kind: "REPORTS-TO".to_string(),
                    confidence: None,
                },
            ],
        };
        let md = render_entity_markdown(&payload);
        let rec = parse_entity("alice", "entities/alice.md", &md).expect("parses");
        assert_eq!(rec.entity_type, "person");
        assert_eq!(rec.aliases, vec!["Al", "Dr. Smith"]);
        assert_eq!(rec.description, "Engineer on the storage team.");
        assert_eq!(rec.relations.len(), 2);
        assert_eq!(rec.relations[0].target, "nexus");
        assert_eq!(rec.relations[0].kind, "works_on");
        assert!((rec.relations[0].confidence - 0.9).abs() < 1e-5);
        assert_eq!(rec.relations[1].kind, "managed_by");
        assert!((rec.relations[1].confidence - 1.0).abs() < 1e-5);
    }

    #[test]
    fn render_entity_markdown_omits_empty_optional_sections() {
        let payload = EntityUpsert {
            id: "minimal".to_string(),
            entity_type: "concept".to_string(),
            aliases: Vec::new(),
            description: String::new(),
            relations: Vec::new(),
        };
        let md = render_entity_markdown(&payload);
        assert!(!md.contains("aliases:"));
        assert!(!md.contains("description:"));
        assert!(!md.contains("relations:"));
        assert!(md.contains("entity_type: concept"));
    }

    #[test]
    fn render_entity_markdown_clamps_confidence_into_unit_interval() {
        let payload = EntityUpsert {
            id: "x".to_string(),
            entity_type: "person".to_string(),
            aliases: Vec::new(),
            description: String::new(),
            relations: vec![
                EntityUpsertRelation {
                    target: "y".to_string(),
                    kind: "knows".to_string(),
                    confidence: Some(2.5),
                },
                EntityUpsertRelation {
                    target: "z".to_string(),
                    kind: "knows".to_string(),
                    confidence: Some(-0.4),
                },
            ],
        };
        let md = render_entity_markdown(&payload);
        let rec = parse_entity("x", "entities/x.md", &md).expect("parses");
        assert!((rec.relations[0].confidence - 1.0).abs() < 1e-5);
        assert!(rec.relations[1].confidence.abs() < 1e-5);
    }

    // ── BL-129 decay_file_content ───────────────────────────────────────────

    fn entity_src(relations_yaml: &str, body: &str) -> String {
        format!("---\nentity_type: person\ndescription: A person.\n{relations_yaml}---\n{body}")
    }

    #[test]
    fn decay_reduces_confidence_by_factor() {
        let src = entity_src(
            "relations:\n  - target: nexus\n    type: works_on\n    confidence: 1.0\n",
            "",
        );
        let out = decay_file_content(
            &src,
            &DecayParams {
                factor: 0.5,
                floor: 0.1,
            },
        )
        .expect("relation should decay");
        assert_eq!(out.relations_decayed, 1);
        assert_eq!(out.relations_at_floor, 0);
        let rec = parse_entity("e", "entities/e.md", &out.content).expect("parses");
        assert!((rec.relations[0].confidence - 0.5).abs() < 1e-5);
    }

    #[test]
    fn decay_clamps_to_floor_and_counts_at_floor() {
        let src = entity_src(
            "relations:\n  - target: nexus\n    type: works_on\n    confidence: 0.15\n",
            "",
        );
        let out = decay_file_content(
            &src,
            &DecayParams {
                factor: 0.5,
                floor: 0.1,
            },
        )
        .expect("clamps to floor");
        assert_eq!(out.relations_decayed, 1);
        assert_eq!(out.relations_at_floor, 1);
        let rec = parse_entity("e", "entities/e.md", &out.content).expect("parses");
        assert!((rec.relations[0].confidence - 0.1).abs() < 1e-5);
    }

    #[test]
    fn decay_skips_relations_already_at_floor() {
        let src = entity_src(
            "relations:\n  - target: nexus\n    type: works_on\n    confidence: 0.1\n",
            "",
        );
        assert!(decay_file_content(
            &src,
            &DecayParams {
                factor: 0.95,
                floor: 0.1
            }
        )
        .is_none());
    }

    #[test]
    fn decay_no_relations_is_noop() {
        let src = entity_src("", "");
        assert!(decay_file_content(
            &src,
            &DecayParams {
                factor: 0.5,
                floor: 0.1
            }
        )
        .is_none());
    }

    #[test]
    fn decay_factor_one_is_noop() {
        let src = entity_src(
            "relations:\n  - target: nexus\n    type: works_on\n    confidence: 1.0\n",
            "",
        );
        assert!(decay_file_content(
            &src,
            &DecayParams {
                factor: 1.0,
                floor: 0.1
            }
        )
        .is_none());
    }

    #[test]
    fn decay_preserves_body() {
        let src = entity_src(
            "relations:\n  - target: nexus\n    type: works_on\n    confidence: 1.0\n",
            "Free-form notes about the entity.\n\nA second paragraph.\n",
        );
        let out = decay_file_content(
            &src,
            &DecayParams {
                factor: 0.5,
                floor: 0.1,
            },
        )
        .expect("relation should decay");
        assert!(out.content.contains("Free-form notes about the entity."));
        assert!(out.content.contains("A second paragraph."));
    }

    #[test]
    fn decay_no_frontmatter_is_noop() {
        let src = "Just a body, no frontmatter.\n";
        assert!(decay_file_content(
            src,
            &DecayParams {
                factor: 0.5,
                floor: 0.1
            }
        )
        .is_none());
    }

    // ── BL-129 merge_records ────────────────────────────────────────────────

    fn rec(
        id: &str,
        entity_type: &str,
        aliases: &[&str],
        description: &str,
        relations: &[(&str, &str, f32)],
    ) -> EntityRecord {
        EntityRecord {
            id: id.to_string(),
            entity_type: entity_type.to_string(),
            aliases: aliases.iter().map(|s| (*s).to_string()).collect(),
            description: description.to_string(),
            relations: relations
                .iter()
                .map(|(t, k, c)| EntityRelation {
                    target: (*t).to_string(),
                    kind: (*k).to_string(),
                    confidence: *c,
                })
                .collect(),
            relpath: format!("entities/{id}.md"),
        }
    }

    #[test]
    fn merge_unions_aliases_and_appends_dropped_id() {
        let keep = rec("alice", "person", &["Al"], "Engineer.", &[]);
        let drop = rec("alice-2", "person", &["Alice S."], "", &[]);
        let m = merge_records(&keep, &drop);
        assert!(m.payload.aliases.contains(&"Al".to_string()));
        assert!(m.payload.aliases.contains(&"Alice S.".to_string()));
        assert!(m.payload.aliases.contains(&"alice-2".to_string()));
        // 'Alice S.' + 'alice-2' both new on keep
        assert_eq!(m.aliases_added, 2);
    }

    #[test]
    fn merge_dedupes_aliases_case_insensitively() {
        let keep = rec("alice", "person", &["Al"], "", &[]);
        let drop = rec("alice-2", "person", &["AL", "Alice"], "", &[]);
        let m = merge_records(&keep, &drop);
        // Already-present "AL" matches "Al"; "Alice" + "alice-2" added.
        assert_eq!(m.aliases_added, 2);
    }

    #[test]
    fn merge_picks_longer_description() {
        let keep = rec("a", "person", &[], "short", &[]);
        let drop = rec("b", "person", &[], "much longer description", &[]);
        let m = merge_records(&keep, &drop);
        assert_eq!(m.payload.description, "much longer description");
    }

    #[test]
    fn merge_unions_relations_with_max_confidence() {
        let keep = rec(
            "a",
            "person",
            &[],
            "",
            &[("x", "knows", 0.4), ("y", "knows", 0.9)],
        );
        let drop = rec(
            "b",
            "person",
            &[],
            "",
            &[
                ("x", "knows", 0.7),    // dup — should keep 0.7 (max)
                ("z", "works_on", 0.5), // new
            ],
        );
        let m = merge_records(&keep, &drop);
        assert_eq!(m.relations_added, 1); // only z is new
        let x = m
            .payload
            .relations
            .iter()
            .find(|r| r.target == "x")
            .unwrap();
        assert!((x.confidence.unwrap() - 0.7).abs() < 1e-5);
        let z = m
            .payload
            .relations
            .iter()
            .find(|r| r.target == "z")
            .unwrap();
        assert!((z.confidence.unwrap() - 0.5).abs() < 1e-5);
    }

    // ── BL-129 follow-up list_draft_relations ──────────────────────────────

    #[test]
    fn list_draft_relations_returns_low_confidence_rows_sorted_ascending() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\nrelations:\n  - target: nexus\n    type: works_on\n    confidence: 0.5\n  - target: bob\n    type: knows\n    confidence: 1.0\n",
            "",
        );
        write_entity(
            dir.path(),
            "bob",
            "entity_type: person\nrelations:\n  - target: alice\n    type: knows\n    confidence: 0.2\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let (rows, total) = index.list_draft_relations(0.5, 10);
        assert_eq!(total, 2);
        assert_eq!(rows.len(), 2);
        // bob → alice (0.2) sorts before alice → nexus (0.5)
        assert_eq!(rows[0].from, "bob");
        assert_eq!(rows[0].target, "alice");
        assert!((rows[0].confidence - 0.2).abs() < 1e-5);
        assert_eq!(rows[1].from, "alice");
        assert_eq!(rows[1].target, "nexus");
        assert!((rows[1].confidence - 0.5).abs() < 1e-5);
    }

    #[test]
    fn list_draft_relations_excludes_confirmed_relations() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\nrelations:\n  - target: nexus\n    type: works_on\n    confidence: 1.0\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let (rows, total) = index.list_draft_relations(0.5, 10);
        assert!(rows.is_empty());
        assert_eq!(total, 0);
    }

    #[test]
    fn list_draft_relations_threshold_is_inclusive() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\nrelations:\n  - target: nexus\n    type: works_on\n    confidence: 0.5\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let (rows, total) = index.list_draft_relations(0.5, 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(total, 1);
    }

    #[test]
    fn list_draft_relations_respects_limit_and_reports_total() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\nrelations:\n  - target: a\n    type: knows\n    confidence: 0.1\n  - target: b\n    type: knows\n    confidence: 0.2\n  - target: c\n    type: knows\n    confidence: 0.3\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let (rows, total) = index.list_draft_relations(0.5, 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(total, 3);
        // sorted ascending — first two are the lowest-confidence rows
        assert_eq!(rows[0].target, "a");
        assert_eq!(rows[1].target, "b");
    }

    #[test]
    fn list_draft_relations_carries_source_relpath() {
        let dir = tempdir();
        write_entity(
            dir.path(),
            "alice",
            "entity_type: person\nrelations:\n  - target: bob\n    type: knows\n    confidence: 0.5\n",
            "",
        );
        let index = EntityIndex::load(dir.path());
        let (rows, _) = index.list_draft_relations(0.5, 10);
        assert_eq!(rows[0].relpath, "entities/alice.md");
    }

    #[test]
    fn list_draft_relations_empty_index_returns_zero() {
        let dir = tempdir();
        let index = EntityIndex::load(dir.path());
        let (rows, total) = index.list_draft_relations(0.5, 10);
        assert!(rows.is_empty());
        assert_eq!(total, 0);
    }

    #[test]
    fn decay_is_idempotent_after_reaching_floor() {
        // Iterate the pass; the second call (after the relation
        // clamps) must report no change.
        let src = entity_src(
            "relations:\n  - target: nexus\n    type: works_on\n    confidence: 0.15\n",
            "body",
        );
        let first = decay_file_content(
            &src,
            &DecayParams {
                factor: 0.5,
                floor: 0.1,
            },
        )
        .expect("first pass decays");
        assert!(decay_file_content(
            &first.content,
            &DecayParams {
                factor: 0.5,
                floor: 0.1
            }
        )
        .is_none());
    }
}
