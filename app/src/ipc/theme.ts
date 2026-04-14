// Typed wrappers for the nexus-theme Tauri commands.
//
// Field shapes are generated from the Rust structs by `ts-rs`; regenerate
// with `cargo test -p nexus-theme export_bindings` after changing any
// annotated type.

import { invoke } from "@tauri-apps/api/core";
import type {
  AppliedTheme,
  SnippetMetadata,
  ThemeConfig,
  ThemeMetadata,
} from "../bindings";

export type {
  AppliedTheme,
  SnippetMetadata,
  SnippetMode,
  SnippetScope,
  ThemeCategory,
  ThemeConfig,
  ThemeMetadata,
  ThemeMode,
} from "../bindings";

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
