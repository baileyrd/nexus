// shell/src/plugins/nexus/ai/contextContributors.ts
//
// BL-032 — context-contributor registry behind the Cmd+I overlay.
//
// Surfaces (editor, bases, canvas, terminal, …) register an adapter
// that, when invoked, returns the snippet of "what the user is looking
// at right now" they want to feed into the AI prompt. The overlay
// invokes every registered contributor on activation, renders the
// returned chips for visibility (BL-033 will style them), and prepends
// the concatenated `promptBlock`s to the user's free-form question
// before routing it through the existing `com.nexus.ai::stream_ask`
// IPC.
//
// Adapter contract is async + null-returning so a contributor that
// doesn't have anything to say (e.g. editor adapter when no tab is
// focused) just returns `null` and falls out of the assembled prompt.
//
// Mirrors the singleton-module pattern used by `fencedCodeRegistry`
// (BL-008). Lives in the ai/ directory so it ships alongside the
// overlay UI without spinning up a new public extension-api surface;
// promotion to `api.ai.registerContextContributor` is left to the
// follow-up that lands BL-033.

/** Categorical hint for how a contributed chip should render. */
export type ContextChipKind =
  | 'file'
  | 'selection'
  | 'block'
  | 'row'
  | 'node'
  | 'query'
  | 'note'

/** A single visible chip surfaced inside the overlay's "what's in the
 *  prompt" rail. Click-to-remove lands with BL-033; v1 just renders. */
export interface ContextChip {
  /** Stable id within a contribution; used as a React key and the
   *  click-target for click-to-remove. */
  id: string
  /** Short label rendered on the chip ("Selection · 42 lines",
   *  "README.md", …). Kept tight to fit the overlay rail. */
  label: string
  kind: ContextChipKind
}

/** What a single surface returned for the current Cmd+I activation. */
export interface ContextContribution {
  /** Surface id the contribution came from. Doubles as a chip group
   *  label when multiple surfaces fire (uncommon — usually only one
   *  surface is active). */
  surfaceId: string
  /** Visible chips. May be empty when the contributor wants to inject
   *  prompt text without surfacing a chip (rare). */
  chips: ContextChip[]
  /** Markdown block prepended to the user's free-form question.
   *  Optional — a contributor may surface chips for awareness only and
   *  rely on a sibling adapter for the actual prompt body. */
  promptBlock?: string
}

/** Async adapter the surface plugin registers. The overlay invokes
 *  every adapter on activation; null/empty results are dropped. */
export type ContextContributor = () =>
  | ContextContribution
  | null
  | Promise<ContextContribution | null>

interface Entry {
  surfaceId: string
  contributor: ContextContributor
}

class ContextContributorRegistry {
  private entries: Entry[] = []

  /** Register a contributor for a surface. Returns an idempotent
   *  disposer; the AI plugin tracks it through `PluginRegistry`'s
   *  subscription sweep so unloads clean up automatically. */
  register(surfaceId: string, contributor: ContextContributor): () => void {
    const id = surfaceId.trim()
    if (!id) {
      console.warn(`[contextContributors] register: empty surfaceId — ignored`)
      return () => {}
    }
    const entry: Entry = { surfaceId: id, contributor }
    this.entries.push(entry)
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      const idx = this.entries.indexOf(entry)
      if (idx !== -1) this.entries.splice(idx, 1)
    }
  }

  /** Snapshot of registered entries, in registration order. Exposed
   *  for tests; production callers should go through `collect()`. */
  list(): ReadonlyArray<{ surfaceId: string }> {
    return this.entries.map((e) => ({ surfaceId: e.surfaceId }))
  }

  /** Invoke every registered contributor and return the non-null
   *  contributions in registration order. Errors thrown by a single
   *  contributor are logged and skipped — one bad adapter must never
   *  block the overlay from opening. */
  async collect(): Promise<ContextContribution[]> {
    const out: ContextContribution[] = []
    for (const entry of this.entries) {
      try {
        const result = await entry.contributor()
        if (result) out.push(result)
      } catch (err) {
        console.warn(
          `[contextContributors] '${entry.surfaceId}' contributor threw`,
          err,
        )
      }
    }
    return out
  }

  /** Test-only — wipe every registration. Production code never
   *  needs this; the disposer pattern handles teardown. */
  _resetForTests(): void {
    this.entries = []
  }
}

export const contextContributors = new ContextContributorRegistry()

// ── Prompt assembly ───────────────────────────────────────────────────────

/** Final shape handed to the AI runtime: the human prompt, plus the
 *  concatenated context block to splice into the model message. Kept
 *  separate so callers can render the human prompt on screen verbatim
 *  while still feeding the model the augmented version. */
export interface AssembledPrompt {
  /** The user's untouched free-form input. */
  userPrompt: string
  /** All `promptBlock`s joined by blank lines, in surface order, then
   *  the user prompt. Empty when no contributor returned a block. */
  assembled: string
  /** Chips collected across every contribution, flattened, for the
   *  overlay's visible rail. Surface order preserved so editor chips
   *  show before canvas chips when both fire. */
  chips: ContextChip[]
}

/**
 * Build the model-facing message from a free-form user prompt + the
 * contributions snapshot. Called by `cmdIRuntime.submit`.
 *
 * Format is deliberately readable — Markdown headings the model can
 * latch onto, no JSON envelope. Avoids the Pieces "context fence"
 * shape because nexus-ai's `stream_ask` already wraps the body in its
 * own conversation envelope.
 */
export function assemblePrompt(
  userPrompt: string,
  contributions: ContextContribution[],
): AssembledPrompt {
  const trimmed = userPrompt.trim()
  const blocks = contributions
    .map((c) => c.promptBlock?.trim())
    .filter((b): b is string => !!b && b.length > 0)
  const chips = contributions.flatMap((c) => c.chips)

  if (blocks.length === 0) {
    return { userPrompt: trimmed, assembled: trimmed, chips }
  }

  const contextBody = blocks.join('\n\n')
  const assembled = `${contextBody}\n\n## Question\n${trimmed}`
  return { userPrompt: trimmed, assembled, chips }
}
