/**
 * AIG-02 — risk classification for agent tool calls.
 *
 * Maps a (target_plugin_id, command_id) pair to one of four risk
 * levels so the approval card can colour-code calls and the
 * `ask_on_risky` policy can auto-approve safe (read-only) calls
 * without prompting the user every round.
 *
 * The mapping is intentionally conservative: any tool we don't
 * recognise is treated as `write`, never `safe`. New plugins can be
 * added here as their command surface stabilises; community-plugin
 * tools always fall through to `write` until classified.
 */

export type RiskLevel = 'safe' | 'write' | 'exec' | 'network'

interface RiskRule {
  /** Reverse-DNS plugin id, e.g. `com.nexus.storage`. */
  pluginId: string
  /** Set of commands at this level. `'*'` matches every command for
   *  the plugin (used when the entire plugin is uniformly classified). */
  commands: ReadonlySet<string> | '*'
  level: RiskLevel
}

// Order matters: the first matching rule wins. Put the more specific
// per-command sets above any wildcard fall-throughs for the same
// plugin.
const RULES: ReadonlyArray<RiskRule> = [
  // ── Storage — file-as-truth surface ──────────────────────────────
  {
    pluginId: 'com.nexus.storage',
    commands: new Set([
      'read_file',
      'list_dir',
      'list_directory',
      'search',
      'backlinks',
      'outgoing_links',
      'metadata',
      'stat',
    ]),
    level: 'safe',
  },
  {
    pluginId: 'com.nexus.storage',
    commands: '*', // anything else (write_file, delete, rename, etc.)
    level: 'write',
  },
  // ── Git ──────────────────────────────────────────────────────────
  {
    pluginId: 'com.nexus.git',
    commands: new Set(['log', 'status', 'diff', 'show', 'blame', 'branches']),
    level: 'safe',
  },
  {
    pluginId: 'com.nexus.git',
    commands: new Set(['push', 'pull', 'fetch', 'clone']),
    level: 'network',
  },
  {
    pluginId: 'com.nexus.git',
    commands: '*', // commit, reset, checkout, branch -D, etc.
    level: 'write',
  },
  // ── Terminal / process ───────────────────────────────────────────
  { pluginId: 'com.nexus.terminal', commands: '*', level: 'exec' },
  { pluginId: 'com.nexus.processes', commands: '*', level: 'exec' },
  // ── AI surface — model-only, no side effects ─────────────────────
  { pluginId: 'com.nexus.ai', commands: '*', level: 'safe' },
  // ── MCP host — external tool of unknown effect ───────────────────
  {
    pluginId: 'com.nexus.mcp.host',
    commands: new Set(['list_servers', 'get_server', 'list_tools']),
    level: 'safe',
  },
  { pluginId: 'com.nexus.mcp.host', commands: '*', level: 'network' },
  // ── Knowledge graph / KV / DB — read-mostly catalogue surfaces ───
  {
    pluginId: 'com.nexus.kv',
    commands: new Set(['get', 'list', 'has']),
    level: 'safe',
  },
  { pluginId: 'com.nexus.kv', commands: '*', level: 'write' },
  {
    pluginId: 'com.nexus.database',
    commands: new Set(['query', 'select', 'list_tables', 'describe']),
    level: 'safe',
  },
  { pluginId: 'com.nexus.database', commands: '*', level: 'write' },
  // ── Skills / workflow — definitions live in the forge so list/get
  //    are reads, run is a side-effect (it dispatches further tools). ─
  {
    pluginId: 'com.nexus.skills',
    commands: new Set(['list', 'get', 'list_by_context', 'triggered_by', 'render']),
    level: 'safe',
  },
  { pluginId: 'com.nexus.skills', commands: '*', level: 'write' },
  {
    pluginId: 'com.nexus.workflow',
    commands: new Set(['list', 'get', 'validate']),
    level: 'safe',
  },
  { pluginId: 'com.nexus.workflow', commands: '*', level: 'exec' },
]

/**
 * Classify a single proposed tool call. Unrecognised plugins fall
 * through to `write` — the agent can still run them, but the user
 * is asked under `ask_on_risky`.
 */
export function classifyToolCall(
  targetPluginId: string,
  commandId: string,
): RiskLevel {
  for (const rule of RULES) {
    if (rule.pluginId !== targetPluginId) continue
    if (rule.commands === '*') return rule.level
    if (rule.commands.has(commandId)) return rule.level
  }
  return 'write'
}

/** True when *every* tool call in a round is read-only-safe. */
export function isRoundEntirelySafe(
  toolCalls: ReadonlyArray<{ target_plugin_id: string; command_id: string }>,
): boolean {
  if (toolCalls.length === 0) return true
  return toolCalls.every(
    (tc) => classifyToolCall(tc.target_plugin_id, tc.command_id) === 'safe',
  )
}

/** Short human label for a risk level (used in approval-card badges). */
export function riskLabel(level: RiskLevel): string {
  switch (level) {
    case 'safe':
      return 'read'
    case 'write':
      return 'write'
    case 'exec':
      return 'exec'
    case 'network':
      return 'network'
  }
}
