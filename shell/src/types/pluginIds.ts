/**
 * Canonical plugin identifiers — keep in sync with
 * `crates/nexus-types/src/plugin_ids.rs`.
 *
 * Every Nexus plugin is referenced by a reverse-DNS string of the form
 * `com.nexus.<name>`. Centralizing the constants here lets shell-side plugin
 * code stop scattering literal strings and stay aligned with Rust.
 *
 * Drift check: a follow-up will emit this file from the Rust source via
 * ts-rs (see Phase 5 P5-02). For now keep entries alphabetized.
 */

export const PLUGIN_IDS = {
  ACP: 'com.nexus.acp',
  AGENT: 'com.nexus.agent',
  AI: 'com.nexus.ai',
  AI_RUNTIME: 'com.nexus.ai.runtime',
  AUDIO: 'com.nexus.audio',
  CLI: 'com.nexus.cli',
  COLLAB: 'com.nexus.collab',
  COMMENTS: 'com.nexus.comments',
  DAP: 'com.nexus.dap',
  DATABASE: 'com.nexus.database',
  EDITOR: 'com.nexus.editor',
  FORMATS: 'com.nexus.formats',
  GIT: 'com.nexus.git',
  KERNEL: 'com.nexus.kernel',
  KV: 'com.nexus.kv',
  LINKPREVIEW: 'com.nexus.linkpreview',
  LSP: 'com.nexus.lsp',
  MCP: 'com.nexus.mcp.host',
  NOTIFICATIONS: 'com.nexus.notifications',
  SECURITY: 'com.nexus.security',
  SKILLS: 'com.nexus.skills',
  STORAGE: 'com.nexus.storage',
  TEMPLATES: 'com.nexus.templates',
  TERMINAL: 'com.nexus.terminal',
  THEME: 'com.nexus.theme',
  TUI: 'com.nexus.tui',
  WORKFLOW: 'com.nexus.workflow',
} as const

export type PluginId = (typeof PLUGIN_IDS)[keyof typeof PLUGIN_IDS]
