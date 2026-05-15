//! Skills plugin registration.
//!
//! PRD-13 scaffold. Read-mostly surface over `.forge/skills/`. Agents +
//! UI consult it over IPC so no consumer links `nexus-skills` directly.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;
use nexus_skills::SkillsCorePlugin;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    let skills_dir = forge_root.join(".forge").join("skills");
    match nexus_skills::seed_builtins(&skills_dir) {
        Ok(report) if !report.created.is_empty() => tracing::info!(
            path = %skills_dir.display(),
            created = ?report.created,
            skipped = report.skipped.len(),
            "seeded built-in skills"
        ),
        Ok(_) => {}
        Err(err) => tracing::warn!(
            path = %skills_dir.display(),
            %err,
            "failed to seed built-in skills — continuing with whatever is already on disk"
        ),
    }
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.skills",
                "Skills",
                LifecycleFlags::NONE,
                &with_v1_aliases(&[
                    ("list", nexus_skills::HANDLER_LIST),
                    ("get", nexus_skills::HANDLER_GET),
                    (
                        "list_by_context",
                        nexus_skills::HANDLER_LIST_BY_CONTEXT,
                    ),
                    (
                        "triggered_by",
                        nexus_skills::HANDLER_TRIGGERED_BY,
                    ),
                    ("reload", nexus_skills::HANDLER_RELOAD),
                    ("render", nexus_skills::HANDLER_RENDER),
                    ("compose", nexus_skills::HANDLER_COMPOSE),
                    ("invoke", nexus_skills::HANDLER_INVOKE),
                ]),
            ),
            forge_root,
            Box::new(SkillsCorePlugin::open(skills_dir)),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.skills")?;
    Ok(())
}
