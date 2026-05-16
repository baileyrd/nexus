// BL-142 Phase 2b.2 — single bus subscriber that routes
// `com.nexus.terminal.output.<sessionId>` events into the
// per-cell `useReplOutputStore`. Subscribe-once, route-many — the
// shell's plugin init code calls `startReplOutputPump(api.kernel)`
// at activation and the returned `stop` runs at deactivation.
//
// Why a single pump instead of per-cell subscriptions: every cell
// has the same shape ("strip ANSI; append to my buffer"), and one
// subscription with a prefix filter is cheaper than N subscriptions
// for N active REPL cells.

import type { KernelAPI } from '../../../types/plugin.ts'

import { stripAnsi } from './cm/stripAnsi.ts'
import { useReplOutputStore } from './replOutputStore.ts'

/** Wire format for `com.nexus.terminal.output.<sessionId>` events.
 *  Mirrors `nexus_terminal::OutputStreamPayload`. The bus carries
 *  raw bytes; the pump's job is to decode → ANSI-strip → append. */
interface OutputStreamPayload {
  /** Raw bytes appended this round. The kernel publishes
   *  `Vec<u8>` which serde encodes as a JSON array of integers. */
  data: number[]
  seq: number
  ts_ms: number
}

const OUTPUT_TOPIC_PREFIX = 'com.nexus.terminal.output.'

/**
 * Decode a `Vec<u8>` JSON wire form into a UTF-8 string. Lossy:
 * invalid sequences map to U+FFFD — which is fine, the worst
 * outcome is a visible "" in the REPL output rather than a hang
 * or a crash.
 */
export function decodeBytes(bytes: number[]): string {
  const u8 = new Uint8Array(bytes.length)
  for (let i = 0; i < bytes.length; i++) u8[i] = bytes[i] & 0xff
  return new TextDecoder('utf-8', { fatal: false }).decode(u8)
}

/** Pure factor — extract the sessionId from a
 *  `com.nexus.terminal.output.<id>` topic string. Returns `null`
 *  if the topic doesn't carry the expected prefix. */
export function sessionIdFromTopic(topic: string): string | null {
  if (!topic.startsWith(OUTPUT_TOPIC_PREFIX)) return null
  return topic.slice(OUTPUT_TOPIC_PREFIX.length)
}

/**
 * Start the bus pump. Returns an unsubscribe function that the
 * caller invokes at plugin deactivation. Idempotent against
 * subscribe failures — if `api.on(...)` rejects, the returned
 * function is a no-op.
 */
export async function startReplOutputPump(
  api: KernelAPI,
): Promise<() => void> {
  const handler = (topic: string, payload: OutputStreamPayload) => {
    const sessionId = sessionIdFromTopic(topic)
    if (!sessionId) return
    if (!Array.isArray(payload?.data)) return
    const text = stripAnsi(decodeBytes(payload.data))
    if (text.length === 0) return
    useReplOutputStore.getState().append(sessionId, text)
  }
  try {
    const unsub = await api.on<OutputStreamPayload>(
      OUTPUT_TOPIC_PREFIX,
      handler,
    )
    return unsub
  } catch {
    return () => {}
  }
}
