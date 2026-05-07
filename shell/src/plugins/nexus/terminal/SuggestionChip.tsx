// shell/src/plugins/nexus/terminal/SuggestionChip.tsx
//
// BL-064 — single-shot AI suggestion surface below the terminal pane.
//
// Polls `com.nexus.terminal::suggest` every few seconds while the
// terminal pane is mounted with a live session id. When a rule
// matches, renders a dismissible chip with the suggested command +
// the (LLM-enriched, when available) reason. "Run" sends the
// command line through `send_input`; "Dismiss" hides the chip until
// the next rule fires for a different `(rule, line)` pair.
//
// Architecture choices:
// - Polling vs. push: BL-064 doesn't ship a `com.nexus.terminal.events.suggestion`
//   topic; the kernel can't decide when output is "interesting". Client-side
//   polling at 5 s keeps the IPC round-trip rate low while staying responsive
//   to a fresh build error.
// - Rendering: deliberately minimal — one row, no animation, no LLM-fanciness.
//   The chip's job is to surface the suggestion, not to compete with the
//   terminal output.
// - Dedup: a `(source_rule, text)` tuple is the dedupe key. The same rule
//   firing twice on the same suggested command shouldn't re-render; a
//   different rule (or same rule with a different `text`) should.

import { useCallback, useEffect, useRef, useState } from 'react'
import type { KernelAPI } from '../../../types/plugin'
import { Icon } from '../../../icons'

const PLUGIN_ID = 'com.nexus.terminal'
const CMD_SUGGEST = 'suggest'
const CMD_SEND_INPUT = 'send_input'

/// How often the chip polls `suggest`. Long enough that an idle
/// terminal isn't burning IPC; short enough that a fresh error
/// surfaces within a few seconds. Matches the BL-064 DoD's "live
/// terminal panel" feel without being spammy.
const POLL_INTERVAL_MS = 5_000

/// Mirrors `crates/nexus-terminal/src/core_plugin.rs::SuggestResponse`.
/// `null` is returned by the handler when no rule fired.
interface SuggestResponse {
  text: string
  reason: string
  severity: 'info' | 'warning' | 'error'
  source_rule: string
  llm_used: boolean
}

interface SuggestionChipProps {
  kernel: KernelAPI
  /** Active session id, or `null` when no terminal session exists. */
  sessionId: string | null
  /** Notification callback — surfaces "sent to terminal" toasts. Optional. */
  onNotify?: (message: string) => void
}

/// Stable dedupe key for a suggestion. Two responses are "the same
/// suggestion" iff this string matches.
function dedupKey(s: SuggestResponse | null): string | null {
  return s ? `${s.source_rule}::${s.text}` : null
}

export function SuggestionChip({ kernel, sessionId, onNotify }: SuggestionChipProps) {
  const [suggestion, setSuggestion] = useState<SuggestResponse | null>(null)
  const [dismissedKey, setDismissedKey] = useState<string | null>(null)

  // Track in-flight requests via a ref so we don't kick off two
  // overlapping polls if the kernel is slow.
  const inFlight = useRef(false)

  const poll = useCallback(async () => {
    if (!sessionId) return
    if (inFlight.current) return
    if (!(await kernel.available())) return
    inFlight.current = true
    try {
      const resp = await kernel.invoke<SuggestResponse | null>(
        PLUGIN_ID,
        CMD_SUGGEST,
        { session_id: sessionId },
      )
      setSuggestion(resp)
    } catch {
      // Failures are silent — the chip just won't render. The next
      // poll round will retry. This matches the kernel-side fallback
      // pattern: when the LLM call times out, the response is the
      // static rule reason; when the IPC itself fails, we hide the
      // chip rather than show a confusing error.
      setSuggestion(null)
    } finally {
      inFlight.current = false
    }
  }, [kernel, sessionId])

  useEffect(() => {
    if (!sessionId) {
      setSuggestion(null)
      setDismissedKey(null)
      return
    }
    // Fire one poll immediately so a fresh session gets a chip
    // within milliseconds rather than waiting for the first interval.
    void poll()
    const handle = setInterval(() => void poll(), POLL_INTERVAL_MS)
    return () => clearInterval(handle)
  }, [poll, sessionId])

  const runSuggested = useCallback(async () => {
    if (!suggestion || !sessionId) return
    try {
      await kernel.invoke(PLUGIN_ID, CMD_SEND_INPUT, {
        id: sessionId,
        input: suggestion.text,
      })
      onNotify?.(`Sent "${suggestion.text}" to terminal`)
    } catch {
      // Same silent-failure rationale as poll: a "send_input failed"
      // toast adds noise without giving the user actionable info.
    }
  }, [kernel, sessionId, suggestion, onNotify])

  const dismiss = useCallback(() => {
    setDismissedKey(dedupKey(suggestion))
  }, [suggestion])

  // Hide cases:
  // - No suggestion yet (handler returned null or poll hasn't fired).
  // - The user dismissed this exact suggestion.
  if (!suggestion) return null
  if (dedupKey(suggestion) === dismissedKey) return null

  const accent =
    suggestion.severity === 'error'
      ? 'var(--text-error, #c53030)'
      : suggestion.severity === 'warning'
        ? 'var(--text-warning, #b7791f)'
        : 'var(--text-faint)'

  return (
    <div
      className="nexus-terminal-suggestion-chip"
      role="status"
      aria-label={`Terminal suggestion from rule ${suggestion.source_rule}`}
    >
      <span
        className="nexus-terminal-suggestion-chip-marker"
        style={{ color: accent }}
        title={`${suggestion.source_rule}${suggestion.llm_used ? ' (AI-enriched)' : ''}`}
      >
        {suggestion.llm_used ? '✨' : '•'}
      </span>
      <code className="nexus-terminal-suggestion-chip-cmd">{suggestion.text}</code>
      <span className="nexus-terminal-suggestion-chip-reason">{suggestion.reason}</span>
      <button
        type="button"
        className="nexus-terminal-suggestion-chip-run"
        onClick={() => void runSuggested()}
        title="Run suggested command in terminal"
        aria-label="Run suggested command"
      >
        <Icon name="play" size={12} />
        <span>Run</span>
      </button>
      <button
        type="button"
        className="nexus-terminal-suggestion-chip-dismiss"
        onClick={dismiss}
        title="Dismiss"
        aria-label="Dismiss suggestion"
      >
        <Icon name="x" size={12} />
      </button>
    </div>
  )
}
