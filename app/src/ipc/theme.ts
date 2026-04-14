// Type-safe wrappers for the nexus-theme Tauri commands.
//
// Field shapes are kept in sync with the Rust structs in
// crates/nexus-theme/src/{theme,api,manifest,snippet}.rs. When that drift
// bites, generate these via `ts-rs` instead of maintaining by hand.

import { invoke } from "@tauri-apps/api/core";

export type ThemeCategory =
  | "light"
  | "dark"
  | "sepia"
  | "high-contrast"
  | "custom";

export interface ThemeMetadata {
  id: string;
  name: string;
  author: string;
  description: string;
  category: ThemeCategory;
  builtin: boolean;
  keywords: string[];
}

export interface AppliedTheme {
  id: string;
  name: string;
  variables: Record<string, string>;
}

export type SnippetMode = "all" | "light" | "dark";

export type SnippetScope = "global" | { "per-surface": string };

export interface SnippetMetadata {
  id: string;
  name: string;
  description: string;
  mode: SnippetMode;
  scope: SnippetScope;
  enabled: boolean;
}

export interface ThemeConfig {
  themeId: string;
  mode: "light" | "dark" | "system";
  enabledSnippets: string[];
}

export function getAvailableThemes(): Promise<ThemeMetadata[]> {
  return invoke("get_available_themes");
}

export function applyTheme(id: string): Promise<AppliedTheme> {
  return invoke("apply_theme", { id });
}

export function computeVariables(
  themeId: string,
  enabledSnippets: string[],
): Promise<Record<string, string>> {
  return invoke("compute_variables", {
    themeId,
    enabledSnippets,
  });
}

export function getAvailableSnippets(): Promise<SnippetMetadata[]> {
  return invoke("get_available_snippets");
}

export function toggleSnippet(id: string): Promise<string[]> {
  return invoke("toggle_snippet", { id });
}

export function reorderSnippets(ids: string[]): Promise<void> {
  return invoke("reorder_snippets", { ids });
}

export function getThemeConfig(): Promise<ThemeConfig> {
  return invoke("get_theme_config");
}
