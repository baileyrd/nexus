// Shared constants for nexus plugins.

/** Wall-clock limit for long-running operations (AI, agent, workflow runs). */
export const LONG_RUNNING_OP_TIMEOUT_MS = 5 * 60_000

/** Wall-clock limit for external service connections (MCP, etc). */
export const SERVICE_CONNECT_TIMEOUT_MS = 60_000

/** Per-tab undo / redo history depth. Shared across canvas + bases so the
 *  user-perceptible "go back" depth is consistent across surfaces. */
export const UNDO_HISTORY_CAP = 200
