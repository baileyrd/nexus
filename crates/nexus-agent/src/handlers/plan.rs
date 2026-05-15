//! `com.nexus.agent::plan` (HANDLER_PLAN).

use std::sync::Arc;

use nexus_kernel::KernelPluginContext;
use nexus_plugins::PluginError;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{build_archetype, Agent, Plan, DEFAULT_SYSTEM_PROMPT};

use super::shared::{
    agent_err, parse, resolve_archetype_for_run, system_prompt_with_skills, to_value,
    AiChatBridge, ArchetypeSource, ResolvedArchetype, DEFAULT_CHAT_TIMEOUT, PLUGIN_ID,
};

/// Args for `com.nexus.agent::plan` and `::run` (handler ids `1`, `2`).
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GoalArgs {
    goal: String,
    #[serde(default)]
    archetype: Option<String>,
}

/// Args for `com.nexus.agent::run_plan` (handler id `7`).
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct PlanArgs {
    plan: Plan,
}

pub(crate) async fn handle_plan(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: GoalArgs = parse(args, "plan")?;
    let skills_prompt =
        system_prompt_with_skills(&ctx, &a.goal, a.archetype.as_deref()).await;
    let extra = skills_prompt
        .strip_prefix(DEFAULT_SYSTEM_PROMPT)
        .map(str::trim_start)
        .filter(|s| !s.is_empty());
    let ResolvedArchetype {
        agent_id,
        system_prompt: agent_prompt,
        source,
        manifest: _,
    } = resolve_archetype_for_run(&ctx, a.archetype.as_deref()).await;
    let driver = AiChatBridge {
        ctx,
        timeout: DEFAULT_CHAT_TIMEOUT,
    };
    let plan = match source {
        ArchetypeSource::Builtin | ArchetypeSource::Default => {
            build_archetype(a.archetype.as_deref(), driver, extra)
                .plan(&a.goal)
                .await
                .map_err(|e| agent_err(&e))?
        }
        ArchetypeSource::CustomManifest { slug } => {
            tracing::debug!(
                plugin_id = PLUGIN_ID,
                custom_slug = %slug,
                agent_id = %agent_id,
                "DG-36: routing through custom archetype manifest",
            );
            crate::archetypes::build_archetype_with_prompt(agent_id, agent_prompt, driver, extra)
                .plan(&a.goal)
                .await
                .map_err(|e| agent_err(&e))?
        }
    };
    to_value(&plan, "plan")
}
