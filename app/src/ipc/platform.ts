import { invoke } from "@tauri-apps/api/core";

/**
 * Platform descriptor returned by the `get_platform_info` Tauri command
 * (PRD-07 §11). Drives the `data-platform` attribute on the root element
 * and gates chrome-effect plugins.
 */
export interface PlatformInfo {
  os: "macos" | "windows" | "linux" | "unknown";
  arch: string;
  supportsVibrancy: boolean;
}

export function getPlatformInfo(): Promise<PlatformInfo> {
  return invoke<PlatformInfo>("get_platform_info");
}
