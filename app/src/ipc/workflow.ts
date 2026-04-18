// Typed wrappers for the com.nexus.workflow core-plugin Tauri commands.

import { invoke } from "@tauri-apps/api/core";

export interface WorkflowMeta {
  name: string;
  description?: string;
  version?: string;
  author?: string;
  tags?: string[];
}

export interface WorkflowTrigger {
  type: string;
  [key: string]: unknown;
}

export interface WorkflowStep {
  name?: string | null;
  type: string;
  parallel?: boolean;
  on_error?: string | null;
  [key: string]: unknown;
}

export interface Workflow {
  workflow: WorkflowMeta;
  trigger: WorkflowTrigger;
  condition?: { type: string; [key: string]: unknown } | null;
  steps?: WorkflowStep[];
  inputs?: Record<string, unknown>;
  outputs?: Record<string, unknown>;
  error_handling?: Record<string, unknown> | null;
}

export function workflowList(): Promise<Workflow[]> {
  return invoke<Workflow[]>("workflow_list");
}

export function workflowGet(name: string): Promise<Workflow> {
  return invoke<Workflow>("workflow_get", { name });
}

export function workflowReload(): Promise<{ loaded: number }> {
  return invoke("workflow_reload");
}

export function workflowValidate(text: string): Promise<Workflow> {
  return invoke<Workflow>("workflow_validate", { text });
}
