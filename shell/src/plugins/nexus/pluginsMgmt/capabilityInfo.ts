// shell/src/plugins/nexus/pluginsMgmt/capabilityInfo.ts
//
// WI-18 — Capability metadata + risk bucketing for the Plugins UI.
//
// Pure module: no React, no shell singletons. Lives next to the
// pluginsMgmt plugin because both that overlay and the Settings >
// Plugins tab consume it. Bumping the canonical bucket / description
// happens here exactly once.
//
// Risk buckets (per Phase 2 WI-18 spec):
//
//   low (green)    — UI-only, intra-shell pub/sub, intra-shell IPC
//                    (UiNotify, EventsPublish, IpcCall)
//   medium (yellow)— in-forge data access (FsRead, KvRead/Write,
//                    DbQuery/Write) and same-machine HTTP loopback
//                    (NetHttpLocalhost)
//   high (red)     — anything that crosses the forge boundary or the
//                    machine: arbitrary filesystem writes, external
//                    fs reads/writes, arbitrary HTTP, process spawn
//                    (FsWrite, FsReadExternal, FsWriteExternal,
//                    NetHttp, ProcessSpawn)
//
// The `FsWrite` bucketing deviates from a literal "in-forge means
// medium" reading: writes mutate forge contents and could clobber the
// user's notes, so they sit in `high` alongside the external variants.
// Reads stay `medium` because the worst case is a leak, which we
// surface separately at install time via the upcoming consent prompt
// (Phase 3 follow-up).
//
// Future: the kernel could ship `CAPABILITY_INFO` over IPC so risk
// buckets stay in lock-step with `crates/nexus-plugins/src/capability.rs`
// without a manual sync. For Phase 2 hard-coding is fine — the enum
// is small and is generated via ts-rs (Phase 1 WI-20), so a new
// variant fails the typecheck immediately (see the `Record<Capability,
// ...>` exhaustiveness check below).

import type { Capability } from '@nexus/extension-api'

export type RiskLevel = 'low' | 'medium' | 'high'

export interface CapabilityMeta {
  risk: RiskLevel
  /** Short, plain-English description shown in the chip tooltip. */
  description: string
}

/**
 * Canonical bucket + description for every `Capability` variant.
 *
 * The `Record<Capability, CapabilityMeta>` annotation is the
 * exhaustiveness check: when a new variant is added to the
 * generated `Capability` union, TypeScript fails this declaration
 * until a bucket is chosen.
 */
export const CAPABILITY_INFO: Record<Capability, CapabilityMeta> = {
  // ── Low risk (green) ────────────────────────────────────────────
  UiNotify:        { risk: 'low',    description: 'Show desktop notifications' },
  EventsPublish:   { risk: 'low',    description: 'Publish events on the in-shell event bus' },
  IpcCall:         { risk: 'low',    description: 'Call other plugins via in-shell IPC' },

  // ── Medium risk (yellow) ────────────────────────────────────────
  KvRead:          { risk: 'medium', description: 'Read this plugin’s key-value store' },
  KvWrite:         { risk: 'medium', description: 'Write to this plugin’s key-value store' },
  DbQuery:         { risk: 'medium', description: 'Run read-only queries on the forge database' },
  DbWrite:         { risk: 'medium', description: 'Mutate rows in the forge database' },
  FsRead:          { risk: 'medium', description: 'Read files inside this forge' },
  NetHttpLocalhost:{ risk: 'medium', description: 'Make HTTP requests to localhost only' },

  // ── High risk (red) ─────────────────────────────────────────────
  FsWrite:         { risk: 'high',   description: 'Write/modify files inside this forge' },
  FsReadExternal:  { risk: 'high',   description: 'Read files outside this forge' },
  FsWriteExternal: { risk: 'high',   description: 'Write files outside this forge' },
  NetHttp:         { risk: 'high',   description: 'Make HTTP requests to arbitrary hosts' },
  ProcessSpawn:    { risk: 'high',   description: 'Spawn external processes' },

  // ── ai.* — ADR 0022 ─────────────────────────────────────────────
  AiIndex:         { risk: 'low',    description: 'Trigger AI indexing of forge files' },
  AiSessionRead:   { risk: 'low',    description: 'Read persisted chat / agent sessions' },
  AiSessionWrite:  { risk: 'low',    description: 'Write or delete persisted chat / agent sessions' },
  AiChat:          { risk: 'medium', description: 'Invoke AI chat surfaces (model can call tools you grant)' },
  AiActivityWrite: { risk: 'medium', description: 'Mutate the AI activity timeline' },
  AiToolsWrite:    { risk: 'medium', description: 'Let the model see write-capable tools (e.g. write_file)' },
  AiToolsMcp:      { risk: 'medium', description: 'Let the model see MCP-bridged external tools' },
  AiConfigWrite:   { risk: 'high',   description: 'Hot-swap AI provider credentials at runtime' },

  // ── audio.* — BL-117 ────────────────────────────────────────────
  AudioSynthesize: { risk: 'low',    description: 'Play synthesized speech through your speakers' },
  AudioRecord:     { risk: 'high',   description: 'Capture audio from your microphone' },

  // ── ai.runtime.* — BL-134 / ADR 0028 ────────────────────────────
  AiRuntimeObserve: { risk: 'low',    description: 'Read AI runtime task state (list/get/events/pool stats)' },
  AiRuntimeSubmit:  { risk: 'medium', description: 'Submit AI agent tasks to the runtime scheduler' },
  AiRuntimeControl: { risk: 'medium', description: 'Cancel, pause, or resume an in-flight AI runtime task' },

  // ── notifications.inbox.* — BL-136 / ADR 0029 ───────────────────
  NotificationsInboxRead:  { risk: 'low', description: 'Read the persistent notification inbox' },
  NotificationsInboxWrite: { risk: 'low', description: 'Mark notifications read or dismiss them' },

  // ── ADR 0027 — protocol-host contribution surface ───────────────
  ProtocolHostContribute: { risk: 'high', description: 'Contribute MCP/LSP/DAP/ACP servers to the protocol host' },

  // ── P1-01 — keyring + audit-log mutation ────────────────────────
  SecurityWrite:      { risk: 'high', description: 'Write/delete OS keyring entries (passwords, API tokens)' },
  SecurityAuditWrite: { risk: 'high', description: 'Truncate the security audit log' },

  // ── P1-07 — network listener ───────────────────────────────────
  NetworkBind: { risk: 'high', description: 'Bind a network listener (collab relay, etc.)' },
}

/** All capability variants known to the shell, in stable display order. */
export const ALL_CAPABILITIES: Capability[] = Object.keys(CAPABILITY_INFO) as Capability[]

/**
 * Bucket a list of capabilities by risk. Always returns all three
 * keys (callers can `.length`-check without optional chaining).
 * Unknown capability strings are silently dropped — the renderer
 * surfaces a separate "(unknown)" affordance when the entire field
 * is missing from the manifest.
 */
export function bucketByRisk(
  caps: readonly Capability[],
): Record<RiskLevel, Capability[]> {
  const out: Record<RiskLevel, Capability[]> = { low: [], medium: [], high: [] }
  for (const cap of caps) {
    const meta = CAPABILITY_INFO[cap]
    if (!meta) continue
    out[meta.risk].push(cap)
  }
  return out
}

/** Highest risk level present in the list, or null if empty. */
export function highestRisk(caps: readonly Capability[]): RiskLevel | null {
  let seenMedium = false
  let seenLow = false
  for (const cap of caps) {
    const meta = CAPABILITY_INFO[cap]
    if (!meta) continue
    if (meta.risk === 'high') return 'high'
    if (meta.risk === 'medium') seenMedium = true
    if (meta.risk === 'low') seenLow = true
  }
  if (seenMedium) return 'medium'
  if (seenLow) return 'low'
  return null
}

/** Convenience predicate for the "show only high-risk" filter. */
export function hasHighRisk(caps: readonly Capability[]): boolean {
  return highestRisk(caps) === 'high'
}

/**
 * CSS-variable backed colour pair for chip rendering. Theme overrides
 * land via `--ok` / `--warn` / `--risk` (see shell/src/styles/tokens.css);
 * the plain hex fallbacks are used only when those tokens are unset
 * (e.g. early-boot, snapshot tests, plain-html previews).
 */
export function chipColours(risk: RiskLevel): { bg: string; fg: string; border: string } {
  switch (risk) {
    case 'low':
      return {
        bg:     'color-mix(in oklch, var(--ok) 18%, transparent)',
        fg:     'var(--ok)',
        border: 'color-mix(in oklch, var(--ok) 35%, transparent)',
      }
    case 'medium':
      return {
        bg:     'color-mix(in oklch, var(--warn) 18%, transparent)',
        fg:     'var(--warn)',
        border: 'color-mix(in oklch, var(--warn) 35%, transparent)',
      }
    case 'high':
      return {
        bg:     'color-mix(in oklch, var(--risk) 20%, transparent)',
        fg:     'var(--risk)',
        border: 'color-mix(in oklch, var(--risk) 40%, transparent)',
      }
  }
}

/**
 * Parse an unknown manifest field into a `Capability[]`.
 *
 *   - `undefined` / `null`           — returns `null` ("not declared")
 *   - empty array                     — returns `[]` ("declared empty")
 *   - array of known variant strings  — returns the filtered list
 *   - anything else                   — returns `null` (treated as missing)
 *
 * The `null` vs `[]` distinction drives the UI's "(unknown)" vs
 * "(none)" badges — important so the user can tell "this plugin opted
 * out of declaring" apart from "this plugin runs with zero permissions".
 */
export function parseManifestCapabilities(raw: unknown): Capability[] | null {
  if (raw === undefined || raw === null) return null
  if (!Array.isArray(raw)) return null
  const known = new Set(ALL_CAPABILITIES)
  const out: Capability[] = []
  for (const entry of raw) {
    if (typeof entry !== 'string') continue
    if (known.has(entry as Capability)) out.push(entry as Capability)
  }
  return out
}
