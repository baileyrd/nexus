//! BL-021 — Skill `depends_on` composition resolver.
//!
//! Implements the four sub-tasks from PRD-13 §5:
//!
//! 1. Parse `depends_on` (already lives on [`crate::SkillMeta`]).
//! 2. Topological sort with cycle detection across the dep DAG rooted
//!    at one skill id.
//! 3. Prompt-fragment merge in well-defined order — dependencies
//!    layered first, the root skill last, so a child's overrides
//!    appear after the base it builds on. Mirrors how class
//!    inheritance reads top-down.
//! 4. Conflict warnings — non-fatal records the planner / Skills UI
//!    can surface ("two ancestors both pin parameter `tone`",
//!    "ancestor allows tool X but descendant restricts it").
//!
//! The resolver is pure (no I/O) and operates against a snapshot of
//! the [`crate::SkillRegistry`]. Callers that want to compose with
//! the on-disk file as truth should `reload()` the registry first.
//!
//! # Algorithm
//!
//! Iterative DFS from the root building up the visited subgraph
//! (white/gray/black colouring) so a back-edge into a `gray` node
//! produces [`ComposeError::Cycle`] with the offending path. Once
//! the subgraph is collected, a Kahn-style topological pass orders
//! dependencies before dependents. Within a single layer, children
//! are visited in `depends_on` declaration order so the merge is
//! deterministic across runs.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use serde::Serialize;

use crate::{Skill, SkillRegistry};

// White: not yet visited. Gray: in the active DFS stack — a back-edge
// into one of these is a cycle. Black: fully processed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Gray,
    Black,
}

/// One layered fragment in a [`ComposedSkill`]. Carries enough metadata
/// for the UI / planner to label and re-order without re-resolving.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComposedFragment {
    /// Skill id contributing this fragment.
    pub id: String,
    /// Skill `name` from the frontmatter (display label).
    pub name: String,
    /// Body string — currently the verbatim `Skill.body`. Future
    /// extensions may pre-render parameter substitutions; the wire
    /// shape here lets us add fields without breaking consumers.
    pub body: String,
}

/// Conflict surface for non-fatal composition warnings. Each variant
/// is a thing the resolver wanted to flag without aborting — the
/// caller (Skills panel, planner) decides whether to surface them as
/// a banner, a side-panel, or to silently log.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComposeConflict {
    /// Two ancestors declared a parameter with the same `name` but a
    /// differing `param_type` or `default`. Last-writer-wins on the
    /// runtime side; the warning surfaces the divergence so the user
    /// can deduplicate.
    ParameterClash {
        /// Parameter name involved in the clash.
        parameter: String,
        /// Skill ids that contributed this name.
        skill_ids: Vec<String>,
    },
    /// One ancestor allowed a tool another ancestor explicitly didn't
    /// list. We don't try to compute set-intersection of `allowed_tools`
    /// here — just flag that two restriction blocks disagree.
    RestrictionsDisagree {
        /// Field that disagrees (`modify_files`, `delete_content`,
        /// `execute_code`, or `allowed_tools`).
        field: String,
        /// Skill ids contributing the differing values.
        skill_ids: Vec<String>,
    },
}

/// Result of resolving a skill's full dependency closure.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComposedSkill {
    /// The root skill id the composition was rooted at.
    pub root_id: String,
    /// Topologically sorted fragments. The root is last; the deepest
    /// dependency is first. Ties broken by `depends_on` declaration
    /// order so the output is stable across runs.
    pub fragments: Vec<ComposedFragment>,
    /// Concatenation of every fragment body, joined by `\n\n` plus a
    /// per-fragment heading so the model can latch onto the boundary.
    /// Format:
    ///
    /// ```text
    /// ## Skill: <name> [<id>]
    /// <body>
    /// ```
    pub merged_body: String,
    /// Non-fatal warnings the UI can surface.
    pub conflicts: Vec<ComposeConflict>,
}

/// Errors that abort composition. Cycles + missing deps are both
/// authoritative failures — a partial composition would be misleading.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ComposeError {
    /// Root skill id wasn't in the registry.
    #[error("no skill with id '{0}' in registry")]
    UnknownRoot(String),
    /// `depends_on` referenced an id that isn't loaded.
    #[error("skill '{from}' depends_on missing skill '{missing}'")]
    MissingDependency {
        /// The skill whose `depends_on` was unresolvable.
        from: String,
        /// The id that wasn't in the registry.
        missing: String,
    },
    /// Found a cycle in the dependency graph.
    #[error("cycle detected: {}", .path.join(" → "))]
    Cycle {
        /// Skill ids forming the cycle, with the back-edge target
        /// repeated at the end so the loop is visually obvious.
        path: Vec<String>,
    },
}

fn visit(
    id: &str,
    registry: &SkillRegistry,
    color: &mut HashMap<String, Color>,
    order: &mut Vec<String>,
    stack: &mut Vec<String>,
) -> Result<(), ComposeError> {
    match color.get(id) {
        Some(Color::Black) => return Ok(()),
        Some(Color::Gray) => {
            // Back-edge — reconstruct the cycle path from the active
            // stack so the operator can see the offending loop.
            let cycle_start = stack.iter().position(|s| s == id).unwrap_or(0);
            let mut path: Vec<String> = stack[cycle_start..].to_vec();
            path.push(id.to_string());
            return Err(ComposeError::Cycle { path });
        }
        None => {}
    }
    let Some(skill) = registry.get(id) else {
        let from = stack.last().cloned().unwrap_or_else(|| id.to_string());
        return Err(ComposeError::MissingDependency {
            from,
            missing: id.to_string(),
        });
    };
    color.insert(id.to_string(), Color::Gray);
    stack.push(id.to_string());
    for dep in &skill.meta.depends_on {
        visit(dep, registry, color, order, stack)?;
    }
    stack.pop();
    color.insert(id.to_string(), Color::Black);
    order.push(id.to_string());
    Ok(())
}

/// Resolve `root_id`'s dependency closure into an ordered, merged
/// composition.
///
/// # Errors
/// - [`ComposeError::UnknownRoot`] if `root_id` isn't loaded.
/// - [`ComposeError::MissingDependency`] for an unresolvable id.
/// - [`ComposeError::Cycle`] for any back-edge in the closure.
pub fn compose(registry: &SkillRegistry, root_id: &str) -> Result<ComposedSkill, ComposeError> {
    if registry.get(root_id).is_none() {
        return Err(ComposeError::UnknownRoot(root_id.to_string()));
    }

    let mut color: HashMap<String, Color> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    visit(root_id, registry, &mut color, &mut order, &mut stack)?;

    // Build fragments in the post-order (deps first, root last). DFS
    // already verified registry membership for every id in `order`,
    // so a missing entry here is a logic bug — surface as `Cycle`
    // with the broken id rather than panicking.
    let mut fragments: Vec<ComposedFragment> = Vec::with_capacity(order.len());
    for id in &order {
        let Some(skill) = registry.get(id) else {
            return Err(ComposeError::MissingDependency {
                from: root_id.to_string(),
                missing: id.clone(),
            });
        };
        fragments.push(ComposedFragment {
            id: skill.meta.id.clone(),
            name: skill.meta.name.clone(),
            body: skill.body.clone(),
        });
    }

    // Merged body: each fragment under a heading the model can latch
    // onto. Empty bodies emit just the heading so the layering is
    // visible even when an ancestor is heading-only.
    let mut merged_body = String::with_capacity(
        fragments
            .iter()
            .map(|f| f.body.len() + f.name.len() + f.id.len() + 32)
            .sum(),
    );
    for (i, f) in fragments.iter().enumerate() {
        if i > 0 {
            merged_body.push_str("\n\n");
        }
        let _ = writeln!(merged_body, "## Skill: {} [{}]", f.name, f.id);
        merged_body.push_str(f.body.trim_end());
    }

    let conflicts = detect_conflicts(registry, &order);

    Ok(ComposedSkill {
        root_id: root_id.to_string(),
        fragments,
        merged_body,
        conflicts,
    })
}

fn lever_check(
    registry: &SkillRegistry,
    order: &[String],
    out: &mut Vec<ComposeConflict>,
    field: &str,
    getter: impl Fn(&Skill) -> Option<bool>,
) {
    let mut by_value: BTreeMap<bool, Vec<String>> = BTreeMap::new();
    for id in order {
        if let Some(skill) = registry.get(id) {
            if let Some(v) = getter(skill) {
                by_value.entry(v).or_default().push(id.clone());
            }
        }
    }
    if by_value.len() > 1 {
        let mut ids: Vec<String> = by_value.values().flatten().cloned().collect();
        ids.sort();
        ids.dedup();
        out.push(ComposeConflict::RestrictionsDisagree {
            field: field.to_string(),
            skill_ids: ids,
        });
    }
}

/// Walk the resolved closure and surface non-fatal conflicts.
fn detect_conflicts(registry: &SkillRegistry, order: &[String]) -> Vec<ComposeConflict> {
    let mut out: Vec<ComposeConflict> = Vec::new();

    // ── parameter clashes ──
    // Group declarations by parameter name; flag any name that has >1
    // distinct (param_type, default) tuple across the closure.
    let mut by_param: BTreeMap<String, Vec<(String, String, Option<String>)>> = BTreeMap::new();
    for id in order {
        if let Some(skill) = registry.get(id) {
            for p in &skill.meta.parameters {
                let default_str = p
                    .default
                    .as_ref()
                    .map(|v| serde_norway::to_string(v).unwrap_or_default());
                by_param.entry(p.name.clone()).or_default().push((
                    id.clone(),
                    p.param_type.clone(),
                    default_str,
                ));
            }
        }
    }
    for (name, decls) in &by_param {
        if decls.len() < 2 {
            continue;
        }
        let first_shape = (&decls[0].1, &decls[0].2);
        let diverges = decls.iter().any(|(_, t, d)| (t, d) != first_shape);
        if diverges {
            let mut ids: Vec<String> = decls.iter().map(|(id, _, _)| id.clone()).collect();
            ids.sort();
            ids.dedup();
            out.push(ComposeConflict::ParameterClash {
                parameter: name.clone(),
                skill_ids: ids,
            });
        }
    }

    // ── restriction disagreements ──
    // For each lever, collect the set of (id, value) pairs and flag
    // when distinct values are present. `allowed_tools` is compared
    // by sorted set-equality.
    lever_check(registry, order, &mut out, "modify_files", |s| {
        s.meta.restrictions.as_ref().and_then(|r| r.modify_files)
    });
    lever_check(registry, order, &mut out, "delete_content", |s| {
        s.meta.restrictions.as_ref().and_then(|r| r.delete_content)
    });
    lever_check(registry, order, &mut out, "execute_code", |s| {
        s.meta.restrictions.as_ref().and_then(|r| r.execute_code)
    });

    // allowed_tools — tolerate empty (== unconstrained) but flag any
    // pair of non-empty allowlists that don't match.
    let mut tool_sets: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    for id in order {
        if let Some(skill) = registry.get(id) {
            if let Some(r) = skill.meta.restrictions.as_ref() {
                if !r.allowed_tools.is_empty() {
                    let mut sorted = r.allowed_tools.clone();
                    sorted.sort();
                    sorted.dedup();
                    tool_sets.entry(sorted).or_default().push(id.clone());
                }
            }
        }
    }
    if tool_sets.len() > 1 {
        let mut ids: Vec<String> = tool_sets.values().flatten().cloned().collect();
        ids.sort();
        ids.dedup();
        out.push(ComposeConflict::RestrictionsDisagree {
            field: "allowed_tools".to_string(),
            skill_ids: ids,
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Skill, SkillMeta};

    fn skill(id: &str, deps: &[&str], body: &str) -> Skill {
        Skill {
            meta: SkillMeta {
                name: id.to_uppercase(),
                id: id.to_string(),
                description: "t".into(),
                version: "1".into(),
                author: "t".into(),
                created: "2026".into(),
                tags: Vec::new(),
                applicable_contexts: Vec::new(),
                triggers: Vec::new(),
                parameters: Vec::new(),
                depends_on: deps.iter().map(|s| (*s).to_string()).collect(),
                restrictions: None,
                output_format: None,
                visibility: None,
                extra: BTreeMap::new(),
            },
            body: body.to_string(),
        }
    }

    fn registry_with(skills: Vec<Skill>) -> SkillRegistry {
        let mut reg = SkillRegistry::empty();
        for s in skills {
            reg.insert(std::path::PathBuf::new(), s);
        }
        reg
    }

    #[test]
    fn compose_returns_self_only_when_no_deps() {
        let reg = registry_with(vec![skill("a", &[], "BODY_A")]);
        let out = compose(&reg, "a").unwrap();
        assert_eq!(out.fragments.len(), 1);
        assert_eq!(out.fragments[0].id, "a");
        assert!(out.merged_body.contains("BODY_A"));
        assert!(out.conflicts.is_empty());
    }

    #[test]
    fn compose_orders_deps_before_dependents() {
        // c depends on b depends on a — root is c.
        let reg = registry_with(vec![
            skill("a", &[], "BODY_A"),
            skill("b", &["a"], "BODY_B"),
            skill("c", &["b"], "BODY_C"),
        ]);
        let out = compose(&reg, "c").unwrap();
        let order: Vec<&str> = out.fragments.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(order, vec!["a", "b", "c"]);
        // Merged body has a's heading first, c's last.
        let pos_a = out.merged_body.find("[a]").unwrap();
        let pos_b = out.merged_body.find("[b]").unwrap();
        let pos_c = out.merged_body.find("[c]").unwrap();
        assert!(pos_a < pos_b && pos_b < pos_c);
    }

    #[test]
    fn compose_diamond_dedupes_to_single_visit() {
        // d depends on b + c; b + c both depend on a. a appears once.
        let reg = registry_with(vec![
            skill("a", &[], "A"),
            skill("b", &["a"], "B"),
            skill("c", &["a"], "C"),
            skill("d", &["b", "c"], "D"),
        ]);
        let out = compose(&reg, "d").unwrap();
        let ids: Vec<&str> = out.fragments.iter().map(|f| f.id.as_str()).collect();
        // 'a' first, 'd' last; b and c in declaration order between.
        assert_eq!(ids[0], "a");
        assert_eq!(ids[ids.len() - 1], "d");
        assert_eq!(ids.len(), 4); // a, b, c, d — no double-a
    }

    #[test]
    fn compose_reports_cycle_with_full_path() {
        // a → b → c → a
        let reg = registry_with(vec![
            skill("a", &["b"], "A"),
            skill("b", &["c"], "B"),
            skill("c", &["a"], "C"),
        ]);
        let err = compose(&reg, "a").unwrap_err();
        match err {
            ComposeError::Cycle { path } => {
                assert_eq!(path.first().map(String::as_str), Some("a"));
                assert_eq!(path.last().map(String::as_str), Some("a"));
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn compose_reports_self_cycle() {
        let reg = registry_with(vec![skill("a", &["a"], "A")]);
        let err = compose(&reg, "a").unwrap_err();
        assert!(matches!(err, ComposeError::Cycle { .. }));
    }

    #[test]
    fn compose_reports_missing_dependency() {
        let reg = registry_with(vec![skill("a", &["nope"], "A")]);
        let err = compose(&reg, "a").unwrap_err();
        match err {
            ComposeError::MissingDependency { from, missing } => {
                assert_eq!(from, "a");
                assert_eq!(missing, "nope");
            }
            other => panic!("expected MissingDependency, got {other:?}"),
        }
    }

    #[test]
    fn compose_reports_unknown_root() {
        let reg = SkillRegistry::empty();
        let err = compose(&reg, "ghost").unwrap_err();
        assert!(matches!(err, ComposeError::UnknownRoot(_)));
    }

    #[test]
    fn compose_surfaces_parameter_clash() {
        use crate::SkillParameter;
        let mut a = skill("a", &[], "A");
        a.meta.parameters.push(SkillParameter {
            name: "tone".into(),
            param_type: "string".into(),
            description: None,
            values: Vec::new(),
            items: None,
            default: Some(serde_norway::Value::String("formal".into())),
        });
        let mut b = skill("b", &["a"], "B");
        b.meta.parameters.push(SkillParameter {
            name: "tone".into(),
            param_type: "string".into(),
            description: None,
            values: Vec::new(),
            items: None,
            default: Some(serde_norway::Value::String("casual".into())),
        });
        let reg = registry_with(vec![a, b]);
        let out = compose(&reg, "b").unwrap();
        let has_clash = out
            .conflicts
            .iter()
            .any(|c| matches!(c, ComposeConflict::ParameterClash { parameter, .. } if parameter == "tone"));
        assert!(
            has_clash,
            "expected ParameterClash, got {:?}",
            out.conflicts
        );
    }

    #[test]
    fn compose_surfaces_restriction_disagreement() {
        use crate::SkillRestrictions;
        let mut a = skill("a", &[], "A");
        a.meta.restrictions = Some(SkillRestrictions {
            modify_files: Some(true),
            ..Default::default()
        });
        let mut b = skill("b", &["a"], "B");
        b.meta.restrictions = Some(SkillRestrictions {
            modify_files: Some(false),
            ..Default::default()
        });
        let reg = registry_with(vec![a, b]);
        let out = compose(&reg, "b").unwrap();
        let has_disagree = out
            .conflicts
            .iter()
            .any(|c| matches!(c, ComposeConflict::RestrictionsDisagree { field, .. } if field == "modify_files"));
        assert!(has_disagree);
    }
}
