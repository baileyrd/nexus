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

export interface ForgeFile {
  relpath: string;
  name: string;
  content: string;
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

export function readForgeFile(relpath: string): Promise<ForgeFile> {
  return invoke("read_forge_file", { relpath });
}

export function writeForgeFile(relpath: string, content: string): Promise<void> {
  return invoke("write_forge_file", { relpath, content });
}

export function createForgeFile(relpath: string): Promise<void> {
  return invoke("create_forge_file", { relpath });
}

export function createForgeDir(relpath: string): Promise<void> {
  return invoke("create_forge_dir", { relpath });
}

export function renameForgeEntry(from: string, to: string): Promise<void> {
  return invoke("rename_forge_entry", { from, to });
}

export function deleteForgeEntry(relpath: string): Promise<void> {
  return invoke("delete_forge_entry", { relpath });
}
