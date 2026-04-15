// Typed wrappers for the forge Tauri commands.
//
// Types are declared locally rather than ts-rs-generated because the
// forge-shell API is a nexus-app concern and small enough that hand-
// written interfaces stay readable.

import { invoke } from "@tauri-apps/api/core";

export interface ForgeInfo {
  name: string;
  root: string;
}

export interface ForgeDirEntry {
  name: string;
  relpath: string;
  isDir: boolean;
}

export function currentForge(): Promise<ForgeInfo | null> {
  return invoke("current_forge");
}

export function openForge(path: string): Promise<ForgeInfo> {
  return invoke("open_forge", { path });
}

export function listForgeDir(relpath: string): Promise<ForgeDirEntry[]> {
  return invoke("list_forge_dir", { relpath });
}
