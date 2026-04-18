// Typed wrappers for the com.nexus.skills core-plugin Tauri commands.
//
// Backend commands live in `nexus-app/src/skills.rs`. All four forward
// to `ipc_call("com.nexus.skills", …)` with a 30-second timeout.

import { invoke } from "@tauri-apps/api/core";

export interface SkillParameter {
  name: string;
  type: string;
  description?: string | null;
  values?: unknown[];
  items?: string | null;
  default?: unknown;
}

export interface SkillRestrictions {
  modify_files?: boolean | null;
  delete_content?: boolean | null;
  execute_code?: boolean | null;
  allowed_tools?: string[];
}

export interface Skill {
  name: string;
  id: string;
  description: string;
  version: string;
  author: string;
  created: string;
  tags?: string[];
  applicable_contexts?: string[];
  triggers?: string[];
  parameters?: SkillParameter[];
  depends_on?: string[];
  restrictions?: SkillRestrictions | null;
  output_format?: string | null;
  visibility?: string | null;
  body: string;
}

export function skillsList(): Promise<Skill[]> {
  return invoke<Skill[]>("skills_list");
}

export function skillsGet(id: string): Promise<Skill> {
  return invoke<Skill>("skills_get", { id });
}

export function skillsRender(
  id: string,
  values?: Record<string, unknown>,
): Promise<{ id: string; name: string; body: string }> {
  return invoke("skills_render", { id, values });
}

export function skillsReload(): Promise<{ loaded: number }> {
  return invoke("skills_reload");
}
