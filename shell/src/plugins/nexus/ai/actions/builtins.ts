// shell/src/plugins/nexus/ai/actions/builtins.ts
//
// BL-035 — built-in AI actions registered by the in-tree AI plugin
// against the shared `aiActionRegistry`. Each action runs against the
// same `com.nexus.ai::stream_chat` handler the chat view uses (with
// `tools: 'auto'` so the BL-016 tool registry is advertised), prompting
// the model with a per-action system message and the
// selection / block payload as the user turn.
//
// Output routing: the four built-ins surface the model's reply through
// `api.notifications` (and a `console.info` mirror for the developer
// console). Splicing the result back into the document is parked
// behind the missing `com.nexus.editor::insert_after_*` IPC — when
// that lands we splice instead of toasting. Until then a toast +
// console line beats silently dropping the answer.

import type { PluginAPI } from '../../../../types/plugin'
import { clientLogger } from '../../../../clientLogger'
import type {
  AiAction,
  AiActionContext,
} from '@nexus/extension-api'
import { streamChat, type StreamChatMessage } from '../aiRuntime'
import { aiActionRegistry } from './registry'

/** Action ids — exported so menu wiring code can reference them
 *  without re-typing the strings. */
export const ACTION_ID_SUMMARIZE = 'nexus.ai.summarize'
export const ACTION_ID_REWRITE = 'nexus.ai.rewrite'
export const ACTION_ID_TRANSLATE = 'nexus.ai.translate'
export const ACTION_ID_EXPLAIN = 'nexus.ai.explain'

/** Default target language for the `translate` action when no user
 *  config override is provided. Kept simple in v1 — BL-035 ships the
 *  registry; per-action settings are deferred. */
const DEFAULT_TARGET_LANGUAGE = 'English'

/** Pull the markdown body out of an action context. Both the editor
 *  selection and block payloads carry plain text we can feed to the
 *  model verbatim; the canvas variant is wired once BL-038 lands. */
function extractText(ctx: AiActionContext): string {
  if (ctx.surface === 'editor.selection') return ctx.selection
  if (ctx.surface === 'block') return ctx.blockText
  return ctx.text
}

/** Short forge-relative description of where the input came from —
 *  threaded into the system prompt so the model has provenance. */
function describeSource(ctx: AiActionContext): string {
  if (ctx.surface === 'editor.selection') {
    return `selection in ${ctx.relpath}`
  }
  if (ctx.surface === 'block') {
    return `block ${ctx.blockId} in ${ctx.relpath}`
  }
  return `canvas node ${ctx.nodeId}`
}

/** Render the model output to the user. Toast + console for v1; once
 *  the editor exposes `insert_after_selection` the inline splice path
 *  takes over and this helper is dropped. */
function surfaceResult(api: PluginAPI, action: AiAction, text: string): void {
  const trimmed = text.trim()
  if (!trimmed) {
    api.notifications.show({
      type: 'warning',
      message: `${action.label}: empty response`,
    })
    return
  }
  // Toast for visibility — long bodies clip but the console line keeps
  // the full payload reachable until we wire chat-panel routing.
  api.notifications.show({
    type: 'info',
    message: `${action.label}: ${trimmed.slice(0, 240)}${trimmed.length > 240 ? '…' : ''}`,
  })
   
  clientLogger.info(`[nexus.ai.actions] ${action.id} →`, trimmed)
}

/** Wrap a prompt-builder + label into a fully-formed `AiAction` bound
 *  to this plugin's `api`. Keeps the four definitions terse. */
function makeAction(
  api: PluginAPI,
  spec: {
    id: string
    label: string
    systemPrompt: string
    /** Build the user-turn body from the surface payload. Defaults
     *  to "{label} the following:\n\n{text}" if omitted. */
    buildUser?: (text: string, ctx: AiActionContext) => string
  },
): AiAction {
  const action: AiAction = {
    id: spec.id,
    label: spec.label,
    surfaces: ['editor.selection', 'block'],
    async run(ctx: AiActionContext): Promise<void> {
      const text = extractText(ctx).trim()
      if (!text) return
      const userBody = spec.buildUser
        ? spec.buildUser(text, ctx)
        : `${spec.label} the following:\n\n${text}`
      const messages: StreamChatMessage[] = [
        { role: 'user', content: userBody },
      ]
      const system = `${spec.systemPrompt}\n\nSource: ${describeSource(ctx)}.`
      try {
        const reply = await streamChat(api, {
          messages,
          system,
          tools: 'auto',
        })
        surfaceResult(api, action, reply)
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `${spec.label} failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    },
  }
  return action
}

/**
 * Build the four shipped built-in actions bound to this plugin's
 * `api`. Returned in registration order; the AI plugin calls
 * {@link registerBuiltinAiActions} which registers them all and hands
 * back a single bulk-disposer.
 */
export function buildBuiltinAiActions(
  api: PluginAPI,
  opts: { targetLanguage?: string } = {},
): AiAction[] {
  const target = opts.targetLanguage ?? DEFAULT_TARGET_LANGUAGE
  return [
    makeAction(api, {
      id: ACTION_ID_SUMMARIZE,
      label: 'Summarize',
      systemPrompt:
        'You are a concise assistant. Summarize the user-provided text in a tight paragraph (3–5 sentences) preserving every key claim. No preamble, no headings.',
    }),
    makeAction(api, {
      id: ACTION_ID_REWRITE,
      label: 'Rewrite',
      systemPrompt:
        'You are a careful editor. Rewrite the user-provided text for clarity and flow without changing the meaning, tone, or structure (keep markdown formatting). Output only the rewritten text.',
    }),
    makeAction(api, {
      id: ACTION_ID_TRANSLATE,
      label: `Translate to ${target}`,
      systemPrompt: `You are a translator. Translate the user-provided text into ${target}, preserving any markdown formatting. Output only the translation, no commentary.`,
      buildUser: (text) => `Translate the following into ${target}:\n\n${text}`,
    }),
    makeAction(api, {
      id: ACTION_ID_EXPLAIN,
      label: 'Explain',
      systemPrompt:
        'You are a patient teacher. Explain the user-provided text plainly, calling out any jargon. Use short paragraphs; markdown allowed.',
    }),
  ]
}

/**
 * Register every built-in action against the shared
 * {@link aiActionRegistry} and return a single disposer that sweeps
 * them all. The AI plugin's `activate` tracks the disposer so a hot
 * plugin-reload doesn't leave duplicates.
 */
export function registerBuiltinAiActions(
  api: PluginAPI,
  opts: { targetLanguage?: string } = {},
): () => void {
  const disposers = buildBuiltinAiActions(api, opts).map((a) =>
    aiActionRegistry.register(a),
  )
  let disposed = false
  return () => {
    if (disposed) return
    disposed = true
    for (const d of disposers) d()
  }
}
