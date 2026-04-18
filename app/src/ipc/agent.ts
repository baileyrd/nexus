// Typed wrappers for the com.nexus.agent core-plugin Tauri commands.
//
// Backend commands live in `nexus-app/src/agent.rs`. All three forward
// to `ipc_call("com.nexus.agent", …)` with a 10-minute timeout.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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

/** Execute a single step of a plan. Callers drive the per-step
 *  approval loop themselves — pass the same `plan` each call and
 *  increment `index` after each successful step. */
export function agentExecuteStep(
  plan: AgentPlan,
  index: number,
): Promise<StepResult> {
  return invoke<StepResult>("agent_execute_step", { plan, index });
}

export interface AgentHistoryEntry {
  plan_id: string;
  goal?: string | null;
  created_at?: string | null;
  success?: boolean | null;
  steps: number;
  bytes: number;
}

export interface AgentHistoryRecord {
  plan_id: string;
  goal?: string | null;
  plan: AgentPlan;
  observation: Observation;
  created_at?: string | null;
}

/** Every persisted plan history saved under
 *  `.forge/agent/history/*.json` — populated automatically on each
 *  `agent_run` / `agent_run_plan` completion. */
export function agentHistoryList(): Promise<AgentHistoryEntry[]> {
  return invoke<AgentHistoryEntry[]>("agent_history_list");
}

export function agentHistoryGet(planId: string): Promise<AgentHistoryRecord> {
  return invoke<AgentHistoryRecord>("agent_history_get", { planId });
}

export function agentHistoryDelete(planId: string): Promise<void> {
  return invoke<void>("agent_history_delete", { planId });
}

// ── Streaming plan-execution events ──────────────────────────────────────
//
// Emitted while `agent_run` / `agent_run_plan` are in flight. Mirror of the
// `com.nexus.agent.{run_start,step_start,step_done,run_done}` kernel-bus
// topics forwarded by `nexus-app::start_agent_event_forwarder`.

export interface AgentRunStart {
  plan_id: string;
  steps: number;
  goal?: string | null;
}

export interface AgentStepStart {
  plan_id: string;
  step_id: string;
  index: number;
  description: string;
}

export interface AgentStepDone {
  plan_id: string;
  step_id: string;
  index: number;
  status: "ok" | "failed" | "skipped";
  error?: string;
}

export interface AgentRunDone {
  plan_id: string;
  success: boolean;
}

export function onAgentRunStart(
  handler: (ev: AgentRunStart) => void,
): Promise<UnlistenFn> {
  return listen<AgentRunStart>("agent:run_start", (e) => handler(e.payload));
}

export function onAgentStepStart(
  handler: (ev: AgentStepStart) => void,
): Promise<UnlistenFn> {
  return listen<AgentStepStart>("agent:step_start", (e) => handler(e.payload));
}

export function onAgentStepDone(
  handler: (ev: AgentStepDone) => void,
): Promise<UnlistenFn> {
  return listen<AgentStepDone>("agent:step_done", (e) => handler(e.payload));
}

export function onAgentRunDone(
  handler: (ev: AgentRunDone) => void,
): Promise<UnlistenFn> {
  return listen<AgentRunDone>("agent:run_done", (e) => handler(e.payload));
}
