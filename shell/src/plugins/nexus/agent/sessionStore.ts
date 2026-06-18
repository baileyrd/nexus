/**
 * Session-driven state for `nexus.agent` (ADR 0024 + 0025 Phase 2).
 *
 * Replaces the pre-2026 plan/observation store with the
 * round-by-round transcript model the agent core plugin now
 * exposes through `com.nexus.agent::session_run`. The shell
 * subscribes to `com.nexus.agent.round_proposed` and posts
 * `round_decide` to drive interactive approval.
 *
 * The store is intentionally a single source of truth for the
 * view layer: every kernel/IPC handler in `index.ts` mutates
 * this store, the React view reads it, and tests can poke the
 * store directly without spinning up the runtime. No persistence
 * lives here — transcripts are fetched on demand via
 * `session_list` / `session_get` and held only as long as the
 * panel needs them.
 */

import { create } from 'zustand'

export const FALLBACK_ARCHETYPES = [
  { id: 'general', label: 'General' },
  { id: 'writer', label: 'Writer' },
  { id: 'coder', label: 'Coder' },
  { id: 'researcher', label: 'Researcher' },
] as const

export interface ArchetypeInfo {
  /** Short id passed back to `session_run` (or `null` for the default). */
  id: string
  /** Capitalised display label for the picker. */
  label: string
}

export interface ToolCallProposal {
  /** Provider-issued id — used to address per-call approvals. */
  id: string
  /** Tool name as advertised in the AI registry. */
  name: string
  /** Resolved IPC dispatch target — surfaced for transparency. */
  target_plugin_id: string
  command_id: string
  /** Decoded args the model emitted. Rendered as JSON in the UI. */
  args: unknown
}

export interface PendingRound {
  /** Session id this round belongs to. Match against `currentSessionId`. */
  sessionId: string
  /** 1-based round index. */
  round: number
  /** Narration text the model emitted alongside the tool calls. */
  text: string
  /** Tool calls awaiting decision. */
  toolCalls: ToolCallProposal[]
  /**
   * Per-tool approval flags, keyed by `ToolCallProposal.id`. Defaults
   * to `true` for every entry on round arrival; the user toggles in
   * the approval card. The submit handler converts the map to the
   * `RoundDecision` wire shape (approve_all / partial / abort).
   */
  approvals: Record<string, boolean>
}

/** One interactive question from a `com.nexus.agent.ask_requested` event. */
export interface AskQuestion {
  /** Caller-chosen id echoed back in the answer. */
  id: string
  /** Question text shown to the user. */
  prompt: string
  /** Selectable options. Empty means free-form text input. */
  options: string[]
  /** Whether more than one option may be selected. */
  multi: boolean
}

/** The user's in-progress answer to one [`AskQuestion`]. */
export interface AskAnswerDraft {
  /** Chosen option labels (radio → one entry, checkbox → many). */
  selected: string[]
  /** Free-form text (for option-less questions). */
  customInput: string
}

/**
 * An interactive prompt awaiting the user's answer. Mirrors
 * [`PendingRound`] but for the `ask` tool: the agent core plugin emits
 * `com.nexus.agent.ask_requested` mid-session, the user fills in answers,
 * and the runtime posts `ask_respond` to unblock the waiting tool call.
 * Addressed by `askId`, not a session id — the event carries no session.
 */
export interface PendingAsk {
  /** Correlates the answer back to the waiting `ask` call. */
  askId: string
  /** Questions to render, in order. */
  questions: AskQuestion[]
  /** Draft answers keyed by [`AskQuestion.id`]. */
  answers: Record<string, AskAnswerDraft>
}

export type SessionOutcome =
  | 'complete'
  | 'aborted'
  | 'errored'
  | 'max_rounds'
  | 'approval_timeout'

export interface ToolCallRecord {
  id: string
  name: string
  approved: boolean
  reason: string
  /** Raw response JSON. Surfaced only as a JSON-stringified preview. */
  response: unknown
  error: string
}

export interface RoundRecord {
  round: number
  text: string
  tool_calls: ToolCallRecord[]
}

/** Summary returned by `session_list` — enough for the sidebar. */
export interface SessionSummary {
  id: string
  goal: string
  started_at: string
  ended_at: string
  outcome: SessionOutcome
}

/** Full transcript returned by `session_get` — same shape as the kernel. */
export interface SessionTranscript {
  id: string
  goal: string
  archetype: string | null
  started_at: string
  ended_at: string
  rounds: RoundRecord[]
  outcome: SessionOutcome
}

export type SessionPhase =
  | 'idle'
  | 'starting'
  | 'awaiting_round'
  | 'awaiting_approval'
  | 'completed'
  | 'errored'

/**
 * AIG-02 — per-session approval policy. The decision is shell-side:
 * the kernel always sends `round_proposed`; the runtime auto-submits
 * `approve_all` when the policy permits, otherwise it surfaces the
 * approval card.
 *
 * - `always_ask`   — every round shows the card (legacy behaviour).
 * - `ask_on_risky` — auto-approve rounds whose tool calls are all
 *   read-only-safe; ask whenever any call writes / execs / hits the
 *   network. Default.
 * - `auto_approve` — never ask. The agent runs to completion. Set
 *   either at session start (composer) or mid-session via the
 *   "Approve & continue" affordance on the approval card.
 */
export type StepPolicy = 'always_ask' | 'ask_on_risky' | 'auto_approve'

export const DEFAULT_STEP_POLICY: StepPolicy = 'ask_on_risky'

export interface AgentSessionState {
  // ── Composer ──────────────────────────────────────────────────────────
  /** Goal text bound to the textarea. */
  goal: string
  /** Archetype id (`null` = default planner). */
  archetype: string | null
  /** Approval policy applied to incoming `round_proposed` events. */
  stepPolicy: StepPolicy
  /** Catalogue resolved from `list_archetypes`; falls back to the
   *  hard-coded set until the IPC call lands. */
  archetypes: ArchetypeInfo[]
  archetypesLoaded: boolean

  // ── Live session ──────────────────────────────────────────────────────
  /** Active session id (set after `session_run` returns / on first
   *  `round_proposed` event for a brand-new session). */
  currentSessionId: string | null
  /** Transcript-so-far for the active session. Each completed round
   *  is appended once `round_decide` resolves. */
  liveTranscript: RoundRecord[]
  /** The last round_proposed event awaiting user decision. `null`
   *  whenever the session isn't paused for approval. */
  pendingRound: PendingRound | null
  /** The last ask_requested event awaiting the user's answers. `null`
   *  whenever no interactive prompt is open. Independent of
   *  `pendingRound` — an `ask` fires mid-round during tool dispatch. */
  pendingAsk: PendingAsk | null
  /** Where the live session is right now. Drives the composer's
   *  enabled/disabled state. */
  phase: SessionPhase
  /** Most recent error surfaced from the kernel (transport, timeout,
   *  decode). Cleared at the start of every new session. */
  liveError: string | null

  // ── History sidebar ───────────────────────────────────────────────────
  /** Past sessions newest-first; populated by `session_list`. */
  sessions: SessionSummary[]
  sessionsLoading: boolean
  sessionsError: string | null
  /** Selected session id from the sidebar; resolved transcript lives
   *  in `selectedTranscript`. */
  selectedSessionId: string | null
  selectedTranscript: SessionTranscript | null
  selectedTranscriptError: string | null

  // ── Mutators ──────────────────────────────────────────────────────────
  setGoal(goal: string): void
  setArchetype(archetype: string | null): void
  setStepPolicy(policy: StepPolicy): void
  setArchetypes(catalogue: ArchetypeInfo[]): void
  beginSession(sessionId: string | null): void
  setPhase(phase: SessionPhase): void
  setLiveError(error: string | null): void
  appendRound(record: RoundRecord): void
  setPendingRound(round: PendingRound | null): void
  toggleApproval(toolUseId: string, approve: boolean): void
  setPendingAsk(ask: PendingAsk | null): void
  updateAskAnswer(questionId: string, patch: Partial<AskAnswerDraft>): void
  finishSession(outcome: SessionOutcome): void
  clearLive(): void
  setSessions(rows: SessionSummary[]): void
  setSessionsLoading(loading: boolean): void
  setSessionsError(error: string | null): void
  setSelectedSession(id: string | null, transcript: SessionTranscript | null, error: string | null): void
  reset(): void
}

const INITIAL: Omit<AgentSessionState, keyof Mutators> = {
  goal: '',
  archetype: null,
  stepPolicy: DEFAULT_STEP_POLICY,
  archetypes: [...FALLBACK_ARCHETYPES],
  archetypesLoaded: false,
  currentSessionId: null,
  liveTranscript: [],
  pendingRound: null,
  pendingAsk: null,
  phase: 'idle',
  liveError: null,
  sessions: [],
  sessionsLoading: false,
  sessionsError: null,
  selectedSessionId: null,
  selectedTranscript: null,
  selectedTranscriptError: null,
}

type Mutators = {
  [K in keyof AgentSessionState as AgentSessionState[K] extends (...a: never[]) => unknown
    ? K
    : never]: AgentSessionState[K]
}

export const useAgentSessionStore = create<AgentSessionState>((set) => ({
  ...INITIAL,
  setGoal: (goal) => set({ goal }),
  setArchetype: (archetype) => set({ archetype }),
  setStepPolicy: (stepPolicy) => set({ stepPolicy }),
  setArchetypes: (catalogue) =>
    set({
      archetypes: catalogue.length > 0 ? catalogue : [...FALLBACK_ARCHETYPES],
      archetypesLoaded: true,
    }),
  beginSession: (sessionId) =>
    set({
      currentSessionId: sessionId,
      liveTranscript: [],
      pendingRound: null,
      pendingAsk: null,
      phase: 'starting',
      liveError: null,
    }),
  setPhase: (phase) => set({ phase }),
  setLiveError: (liveError) => set({ liveError, phase: liveError ? 'errored' : 'idle' }),
  appendRound: (record) =>
    set((s) => ({ liveTranscript: [...s.liveTranscript, record] })),
  setPendingRound: (pendingRound) =>
    set({ pendingRound, phase: pendingRound ? 'awaiting_approval' : 'awaiting_round' }),
  toggleApproval: (toolUseId, approve) =>
    set((s) => {
      if (!s.pendingRound) return s
      return {
        pendingRound: {
          ...s.pendingRound,
          approvals: { ...s.pendingRound.approvals, [toolUseId]: approve },
        },
      }
    }),
  setPendingAsk: (pendingAsk) => set({ pendingAsk }),
  updateAskAnswer: (questionId, patch) =>
    set((s) => {
      if (!s.pendingAsk) return s
      const prev = s.pendingAsk.answers[questionId] ?? { selected: [], customInput: '' }
      return {
        pendingAsk: {
          ...s.pendingAsk,
          answers: { ...s.pendingAsk.answers, [questionId]: { ...prev, ...patch } },
        },
      }
    }),
  finishSession: (outcome) =>
    set({
      phase: outcome === 'errored' ? 'errored' : 'completed',
      pendingRound: null,
      pendingAsk: null,
    }),
  clearLive: () =>
    set({
      currentSessionId: null,
      liveTranscript: [],
      pendingRound: null,
      pendingAsk: null,
      phase: 'idle',
      liveError: null,
    }),
  setSessions: (rows) => set({ sessions: rows }),
  setSessionsLoading: (sessionsLoading) => set({ sessionsLoading }),
  setSessionsError: (sessionsError) => set({ sessionsError }),
  setSelectedSession: (selectedSessionId, selectedTranscript, selectedTranscriptError) =>
    set({ selectedSessionId, selectedTranscript, selectedTranscriptError }),
  reset: () => set({ ...INITIAL, archetypes: [...FALLBACK_ARCHETYPES] }),
}))

// ── Decoders ────────────────────────────────────────────────────────────
//
// All four exist to defend the store from malformed kernel payloads:
// `round_proposed` events arrive as raw JSON, the response of
// `session_run` ships an `AgentSession`-shaped object, and
// `session_list` returns a JSON array of summaries. Decode at the
// boundary; the React view assumes already-shaped data.

export function describeArchetype(id: string): ArchetypeInfo {
  const known = FALLBACK_ARCHETYPES.find((a) => a.id === id)
  if (known) return { id: known.id, label: known.label }
  return { id, label: id.charAt(0).toUpperCase() + id.slice(1) }
}

export function decodeArchetypes(raw: unknown): ArchetypeInfo[] {
  if (!Array.isArray(raw)) return [...FALLBACK_ARCHETYPES]
  const ids: string[] = []
  for (const item of raw) {
    if (typeof item === 'string' && item.length > 0 && !ids.includes(item)) {
      ids.push(item)
    }
  }
  if (ids.length === 0) return [...FALLBACK_ARCHETYPES]
  return ids.map(describeArchetype)
}

export function decodeProposedRound(
  sessionId: string,
  raw: unknown,
): PendingRound | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  const round = typeof r.round === 'number' ? r.round : null
  if (round === null) return null
  const text = typeof r.text === 'string' ? r.text : ''
  const tcRaw = Array.isArray(r.tool_calls) ? r.tool_calls : []
  const toolCalls: ToolCallProposal[] = []
  for (const item of tcRaw) {
    if (!item || typeof item !== 'object') continue
    const t = item as Record<string, unknown>
    const id = typeof t.id === 'string' ? t.id : null
    const name = typeof t.name === 'string' ? t.name : null
    if (!id || !name) continue
    let target = ''
    let cmd = ''
    let args: unknown = null
    const inner = t.tool_call
    if (inner && typeof inner === 'object') {
      const tc = inner as Record<string, unknown>
      target =
        typeof tc.target_plugin_id === 'string' ? tc.target_plugin_id : ''
      cmd = typeof tc.command_id === 'string' ? tc.command_id : ''
      args = tc.args
    }
    toolCalls.push({
      id,
      name,
      target_plugin_id: target,
      command_id: cmd,
      args,
    })
  }
  const approvals: Record<string, boolean> = {}
  for (const tc of toolCalls) approvals[tc.id] = true
  return { sessionId, round, text, toolCalls, approvals }
}

/**
 * Decode a `com.nexus.agent.ask_requested` payload
 * (`{ ask_id, questions: [{ id, prompt, options, multi }] }`) into a
 * [`PendingAsk`]. Returns `null` when the payload lacks an ask id or any
 * usable question. Each question seeds an empty answer draft.
 */
export function decodeAskRequested(raw: unknown): PendingAsk | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  const askId = typeof r.ask_id === 'string' ? r.ask_id : null
  if (!askId) return null
  const qRaw = Array.isArray(r.questions) ? r.questions : []
  const questions: AskQuestion[] = []
  for (const item of qRaw) {
    if (!item || typeof item !== 'object') continue
    const q = item as Record<string, unknown>
    const id = typeof q.id === 'string' ? q.id : null
    const prompt = typeof q.prompt === 'string' ? q.prompt : null
    if (!id || !prompt) continue
    const options = Array.isArray(q.options)
      ? q.options.filter((o): o is string => typeof o === 'string')
      : []
    questions.push({ id, prompt, options, multi: q.multi === true })
  }
  if (questions.length === 0) return null
  const answers: Record<string, AskAnswerDraft> = {}
  for (const q of questions) answers[q.id] = { selected: [], customInput: '' }
  return { askId, questions, answers }
}

const KNOWN_OUTCOMES: SessionOutcome[] = [
  'complete',
  'aborted',
  'errored',
  'max_rounds',
  'approval_timeout',
]
function decodeOutcome(raw: unknown): SessionOutcome {
  if (typeof raw === 'string' && (KNOWN_OUTCOMES as string[]).includes(raw)) {
    return raw as SessionOutcome
  }
  return 'errored'
}

export function decodeTranscript(raw: unknown): SessionTranscript | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  const id = typeof r.id === 'string' ? r.id : null
  const goal = typeof r.goal === 'string' ? r.goal : null
  if (!id || !goal) return null
  const roundsRaw = Array.isArray(r.rounds) ? r.rounds : []
  const rounds: RoundRecord[] = []
  for (const item of roundsRaw) {
    if (!item || typeof item !== 'object') continue
    const ro = item as Record<string, unknown>
    const round = typeof ro.round === 'number' ? ro.round : null
    if (round === null) continue
    const text = typeof ro.text === 'string' ? ro.text : ''
    const tcRaw = Array.isArray(ro.tool_calls) ? ro.tool_calls : []
    const tool_calls: ToolCallRecord[] = []
    for (const tcItem of tcRaw) {
      if (!tcItem || typeof tcItem !== 'object') continue
      const t = tcItem as Record<string, unknown>
      const tcid = typeof t.id === 'string' ? t.id : ''
      const name = typeof t.name === 'string' ? t.name : ''
      tool_calls.push({
        id: tcid,
        name,
        approved: t.approved === true,
        reason: typeof t.reason === 'string' ? t.reason : '',
        response: t.response ?? null,
        error: typeof t.error === 'string' ? t.error : '',
      })
    }
    rounds.push({ round, text, tool_calls })
  }
  return {
    id,
    goal,
    archetype: typeof r.archetype === 'string' ? r.archetype : null,
    started_at: typeof r.started_at === 'string' ? r.started_at : '',
    ended_at: typeof r.ended_at === 'string' ? r.ended_at : '',
    rounds,
    outcome: decodeOutcome(r.outcome),
  }
}

export function decodeSessionList(raw: unknown): SessionSummary[] {
  if (!Array.isArray(raw)) return []
  const out: SessionSummary[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    const id = typeof r.id === 'string' ? r.id : null
    if (!id) continue
    out.push({
      id,
      goal: typeof r.goal === 'string' ? r.goal : '',
      started_at: typeof r.started_at === 'string' ? r.started_at : '',
      ended_at: typeof r.ended_at === 'string' ? r.ended_at : '',
      outcome: decodeOutcome(r.outcome),
    })
  }
  return out
}
