// Typed wrappers for the com.nexus.agent core-plugin Tauri commands.
//
// Backend commands live in `nexus-app/src/agent.rs`. All three forward
// to `ipc_call("com.nexus.agent", …)` with a 10-minute timeout.

import { invoke } from "@tauri-apps/api/core";

export interface ToolCall {
  target_plugin_id: string;
  command_id: string;
  args: unknown;
}

export interface PlanStep {
  id: string;
  description: string;
  tool_call: ToolCall | null;
}

export interface AgentPlan {
  id: string;
  goal: string;
  steps: PlanStep[];
}

export type StepStatus = "ok" | "denied" | "failed" | "skipped";

export interface StepResult {
  step_id: string;
  response: unknown | null;
  status: StepStatus;
}

export interface Observation {
  plan_id: string;
  steps: StepResult[];
  success: boolean;
}

export type AgentArchetype = "general" | "writer" | "coder" | "researcher";

export function agentPlan(
  goal: string,
  archetype?: AgentArchetype,
): Promise<AgentPlan> {
  return invoke<AgentPlan>("agent_plan", { goal, archetype });
}

export function agentRun(
  goal: string,
  archetype?: AgentArchetype,
): Promise<Observation> {
  return invoke<Observation>("agent_run", { goal, archetype });
}

export function agentRunPlan(plan: AgentPlan): Promise<Observation> {
  return invoke<Observation>("agent_run_plan", { plan });
}
