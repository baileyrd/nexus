// shell/src/plugins/nexus/ai/index.ts
//
// WI-01 Slice A — plugin manifest + activation. Wires:
//
//   1. Kernel handle into the runtime module so submitQuestion /
//      hydrateConfig can call api.kernel.invoke.
//   2. AiConfig snapshot fetch (one-shot, on activate).
//   3. The single `com.nexus.ai.stream_*` prefix subscription that
//      routes chunks/done into the store. PluginRegistry sweeps the
//      disposer on plugin unload (commit c4d31d3) — we don't need
//      to track it manually.
//   4. View registration: viewType `ai-chat`, rendered by AiChatView
//      wrapping <ChatView/> with onSend/onCancel/onRetry bound to
//      the runtime functions.
//   5. Activity-bar item + focus/clear commands (preserved from the
//      prior skeleton — the chrome integration is unchanged).
//
// Slices B + C will extend the store + runtime; this manifest stays
// stable.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { useConfigStore } from '../../../stores/configStore'
import { useContextKeyStore } from '../../../host/ContextKeyService'
import { eventBus } from '../../../host/EventBus'
import { ChatView } from './ChatView'
import { aiChatViewCreator } from './AiChatView'
import { CmdIOverlay } from './CmdIOverlay'
import { useAiStore } from './aiStore'
import { useCmdIStore } from './cmdIStore'
import { setCmdIApi } from './cmdIApi'
import { setGhostApi } from './ghostApi'
import { setMarginApi } from './marginApi'
import { openCmdI, routeStreamEvent } from './cmdIRuntime'
import { registerEditorContextAdapter } from './editorContextAdapter'
import { registerBuiltinAiActions } from './actions/builtins'
import {
  setKernel,
  requestFocus,
  hydrateConfig,
  pushUserConfig,
  type AiUserConfig,
  subscribeStream,
  submitQuestion,
  cancelInFlight,
  retryLast,
  loadSessions,
  loadSession,
  deleteSession,
  renameSession,
  saveCurrentSession,
  startNewChat,
  scheduleAutosave,
  flushAutosave,
} from './aiRuntime'

const VIEW_ID = 'nexus.ai.view'
const VIEW_ID_CMD_I_OVERLAY = 'nexus.ai.cmdI.overlay'
const COMMAND_FOCUS = 'nexus.ai.focus'
const COMMAND_CLEAR = 'nexus.ai.clear'
const COMMAND_OPEN_SETTINGS = 'nexus.ai.openSettings'
const COMMAND_CMD_I_OPEN = 'nexus.ai.cmdI.open'
const COMMAND_CMD_I_CLOSE = 'nexus.ai.cmdI.close'
const CONTEXT_KEY_CMD_I_VISIBLE = 'nexus.ai.cmdI.visible'

const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

// Lucide-style "sparkles" glyph — four-point star in a 24x24 box,
// stroke-only to match the iconPath contract used by the other
// activity-bar items.
const AI_ICON_PATH =
  'M12 3l2.4 5.2L20 10l-5.2 2.4L12 18l-2.4-5.6L4 10l5.6-1.8L12 3z'

export const aiPlugin: Plugin = {
  manifest: {
    id: 'nexus.ai',
    name: 'AI Chat',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar'],
    contributes: {
      configuration: {
        pluginId: 'nexus.ai',
        title: 'AI Chat',
        order: 50,
        // Provider settings live in the same section so the existing
        // settings panel auto-renders them. The shell pushes these
        // values to the kernel via `set_config` on activate and on
        // every change — no restart needed.
        schema: [
          {
            key: 'ai.provider',
            title: 'Chat provider',
            description:
              'AI provider used for chat. Leave blank to fall back to environment variables (ANTHROPIC_API_KEY / OPENAI_API_KEY / OLLAMA_BASE_URL).',
            type: 'select' as const,
            default: '',
            options: ['', 'anthropic', 'openai', 'ollama'],
          },
          {
            key: 'ai.model',
            title: 'Chat model',
            description:
              'Optional model override (e.g. claude-sonnet-4-5, gpt-4o-mini, llama3.1). Leave blank for the provider default.',
            type: 'string' as const,
            default: '',
          },
          {
            key: 'ai.apiKey',
            title: 'API key',
            description:
              'Required for Anthropic and OpenAI. Stored locally in this workspace; never sent anywhere except the provider you choose.',
            type: 'password' as const,
            default: '',
          },
          {
            key: 'ai.baseUrl',
            title: 'Base URL',
            description:
              'Override the provider endpoint. For Ollama this points at your local server (default http://localhost:11434).',
            type: 'string' as const,
            default: '',
          },
          {
            key: 'ai.embedProvider',
            title: 'Embedding provider',
            description:
              'Provider for the RAG retrieval embeddings. OpenAI gives higher quality; Ollama runs locally. Leave blank to share the chat provider where supported.',
            type: 'select' as const,
            default: '',
            options: ['', 'openai', 'ollama'],
          },
          {
            key: 'ai.embedApiKey',
            title: 'Embedding API key',
            description:
              'Only required when the embedding provider differs from the chat provider. Otherwise the chat key is reused.',
            type: 'password' as const,
            default: '',
          },
          {
            key: 'ai.embedBaseUrl',
            title: 'Embedding base URL',
            description:
              'Override the embedding endpoint (used for self-hosted Ollama or OpenAI-compatible proxies).',
            type: 'string' as const,
            default: '',
          },
          {
            key: 'ui.copiedNotificationMs',
            title: 'Copy notification duration',
            description: 'Auto-dismiss duration for "Copied" button feedback in milliseconds',
            type: 'number' as const,
            default: 1200,
          },
          // BL-034 — inline ghost completion settings.
          {
            key: 'ai.ghost.enabled',
            title: 'Inline ghost completions',
            description:
              'Show inline AI completions while typing in the editor. Tab accepts the suggestion; Esc dismisses it.',
            type: 'boolean' as const,
            default: true,
          },
          {
            key: 'ai.ghost.debounceMs',
            title: 'Ghost completion debounce (ms)',
            description: 'Quiet-period after a keystroke before requesting a suggestion.',
            type: 'number' as const,
            default: 350,
          },
          {
            key: 'ai.ghost.minChars',
            title: 'Ghost completion minimum prefix',
            description: 'Skip suggestions when fewer than this many characters precede the caret.',
            type: 'number' as const,
            default: 8,
          },
          {
            key: 'ai.ghost.contextChars',
            title: 'Ghost completion context window',
            description:
              'Number of characters before the caret sent to the model as context.',
            type: 'number' as const,
            default: 2000,
          },
          {
            key: 'ai.ghost.maxTokens',
            title: 'Ghost completion max tokens',
            description: 'Generation cap for each ghost suggestion.',
            type: 'number' as const,
            default: 64,
          },
          // BL-036 phase 4 — ambient margin-suggestions / inline
          // correction trigger. Defaults agree with
          // `MARGIN_SUGGEST_DEFAULTS` in `editor/cm/marginSuggestTrigger.ts`
          // by construction; if you change one, change the other.
          // Opt-in by default (each pass is a model call; enabling
          // implicitly would surprise users with provider config
          // dialogs on first idle).
          {
            key: 'ai.marginSuggest.enabled',
            title: 'Ambient margin suggestions',
            description:
              'Run a background AI pass over the active document while you are idle. Surfaces rephrase / tighten / fact-check glyphs in the right margin and wavy underlines for spelling / grammar. Right-click any suggestion for Accept / Dismiss.',
            type: 'boolean' as const,
            default: false,
          },
          {
            key: 'ai.marginSuggest.idleMs',
            title: 'Margin suggest idle (ms)',
            description:
              'Quiet period after a keystroke before a margin-suggestion pass fires. Higher = fewer model calls; lower = faster feedback.',
            type: 'number' as const,
            default: 5000,
          },
          {
            key: 'ai.marginSuggest.minDocChars',
            title: 'Margin suggest minimum doc length',
            description:
              'Skip the pass when the document is shorter than this many characters. Stops the engine from running on near-empty notes.',
            type: 'number' as const,
            default: 200,
          },
          {
            key: 'ai.marginSuggest.maxDocChars',
            title: 'Margin suggest maximum doc length',
            description:
              'Skip the pass when the document is longer than this many characters. Caps token cost on book-length notes.',
            type: 'number' as const,
            default: 8000,
          },
        ],
      },
      commands: [
        { id: COMMAND_FOCUS, title: 'Focus Chat', category: 'AI' },
        { id: COMMAND_CLEAR, title: 'Clear Chat', category: 'AI' },
        { id: COMMAND_OPEN_SETTINGS, title: 'Configure AI provider', category: 'AI' },
        // BL-032 — Cmd+I command-anywhere AI overlay.
        { id: COMMAND_CMD_I_OPEN, title: 'Ask AI about current context…', category: 'AI' },
        { id: COMMAND_CMD_I_CLOSE, title: 'Dismiss AI overlay', category: 'AI' },
      ],
      keybindings: [
        { command: COMMAND_FOCUS, key: 'ctrl+alt+a', mac: 'cmd+alt+a' },
        // Primary BL-032 binding. Stays out of the way of the command
        // palette (Ctrl+Shift+P) and the editor's italic shortcut by
        // using the Pieces / VS Code "Inline Chat" convention.
        { command: COMMAND_CMD_I_OPEN, key: 'ctrl+i', mac: 'cmd+i' },
        // Esc inside the overlay is handled by the component itself
        // (App.tsx short-circuits keybindings while focus is on a
        // textarea), but registering it here makes the close action
        // discoverable in the command palette.
        {
          command: COMMAND_CMD_I_CLOSE,
          key: 'escape',
          when: CONTEXT_KEY_CMD_I_VISIBLE,
        },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_CMD_I_VISIBLE,
          description: 'True while the Cmd+I AI overlay is open.',
          type: 'boolean',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    api.configuration.register(aiPlugin.manifest.contributes!.configuration!)
    setKernel(api.kernel)

    // Bind runtime functions to this plugin's PluginAPI handle so the
    // view can fire them without re-importing the API. Closures keep
    // the wiring local to this file and out of the view component.
    const onSend = (q: string) => submitQuestion(api, q)
    const onCancel = () => cancelInFlight()
    const onRetry = () => retryLast(api)
    // RAG source chips emit `files:open` so the editor plugin opens
    // the cited document. Routed through PluginAPI's event bus, not
    // direct kernel emit — the editor subscribes via `api.events.on`.
    const onEmit = (event: string, payload: unknown) => {
      api.events.emit(event, payload)
    }
    // Slice C session-management bindings. Same closure-over-api
    // pattern as send/cancel/retry above so the view stays decoupled
    // from PluginAPI.
    const onNewChat = () => startNewChat(api)
    const onLoadSession = (id: string) => loadSession(api, id)
    const onDeleteSession = (id: string) => deleteSession(api, id)
    const onRenameSession = (id: string, title: string) =>
      renameSession(api, id, title)
    const onSaveSession = () => saveCurrentSession(api).then(() => undefined)
    const onOpenSettings = () => {
      void api.commands.execute(COMMAND_OPEN_SETTINGS)
    }

    viewRegistry.register(
      'ai-chat',
      aiChatViewCreator(() =>
        createElement(ChatView, {
          onSend,
          onCancel,
          onRetry,
          onEmit,
          onNewChat,
          onLoadSession,
          onDeleteSession,
          onRenameSession,
          onSaveSession,
          onOpenSettings,
        }),
      ),
    )

    api.activityBar.addItem({
      id: 'nexus.ai.activityItem',
      icon: '',
      iconPath: AI_ICON_PATH,
      title: 'AI Chat',
      viewId: VIEW_ID,
      priority: 50,
      command: COMMAND_FOCUS,
    })

    // Focus command — ensure an ai-chat leaf exists on the right and
    // reveal it; the view's mount-time focuser drains pendingFocus.
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType('ai-chat', 'main')
      workspace.revealLeaf(leaf)
      requestFocus()
    })

    // Clear command — wipe the conversation history. We cancel any
    // in-flight stream first so the assistant turn we're about to
    // delete doesn't keep accruing chunks into nothing. `clearTurns`
    // (vs `reset`) preserves the hydrated config + composer text.
    api.commands.register(COMMAND_CLEAR, () => {
      cancelInFlight()
      useAiStore.getState().clearTurns()
    })

    // Wipe the store when the workspace closes. Answers from a
    // previous forge don't belong in a freshly opened one. Don't
    // tear down the subscription — PluginRegistry handles that on
    // plugin unload.
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      cancelInFlight()
      flushAutosave()
      useAiStore.getState().reset()
      useAiStore.setState({ config: null })
    })

    // ── Slice C: auto-save on assistant turn completion ───────────────────
    //
    // Subscribe to the turns array. Whenever the most recent assistant
    // turn flips to status='done' AND we have content worth saving,
    // schedule a debounced save. The debounce collapses streaming
    // bursts into one disk write; per `aiRuntime.AUTOSAVE_DEBOUNCE_MS`
    // the trailing edge wins.
    //
    // Decision (vs legacy ChatPanel.tsx auto-save on every turns.length
    // change): we only fire when an assistant turn finalizes, not on
    // user-turn append. The user turn alone has nothing to recover —
    // it's still in the composer-state-machine until the assistant
    // responds, and a save under just a user prompt would create a
    // titled session with no content if the kernel errors.
    let lastDoneCount = 0
    useAiStore.subscribe((state) => {
      const doneCount = state.turns.reduce(
        (n, t) => (t.kind === 'assistant' && t.status === 'done' ? n + 1 : n),
        0,
      )
      if (doneCount > lastDoneCount && doneCount > 0) {
        scheduleAutosave(api)
      }
      lastDoneCount = doneCount
    })

    // ── AI provider settings ─────────────────────────────────────────────
    //
    // Read user-saved provider settings out of the shell config store
    // and push them to the kernel. The kernel falls back to env-var
    // detection if every field is blank, so a fresh install with
    // ANTHROPIC_API_KEY set keeps working.
    const readUserConfig = (): AiUserConfig => {
      const cfg = useConfigStore.getState()
      return {
        provider: cfg.get<string>('ai.provider', ''),
        model: cfg.get<string>('ai.model', ''),
        apiKey: cfg.get<string>('ai.apiKey', ''),
        baseUrl: cfg.get<string>('ai.baseUrl', ''),
        embedProvider: cfg.get<string>('ai.embedProvider', ''),
        embedApiKey: cfg.get<string>('ai.embedApiKey', ''),
        embedBaseUrl: cfg.get<string>('ai.embedBaseUrl', ''),
      }
    }

    // Open the settings panel and route directly to the AI section.
    // Wired into the chat view's empty state so a fresh user with no
    // provider lands one click from a working chat.
    api.commands.register(COMMAND_OPEN_SETTINGS, () => {
      const cks = useContextKeyStore.getState()
      cks.set('settingsPanelVisible', true)
      cks.set('settingsActiveTab', 'settings')
      // SettingsPanelView reads activeSection from local component
      // state — emit a separate event the panel listens for, so the
      // user lands in the AI section instead of whatever was open.
      eventBus.emit('settings:focusSection', 'nexus.ai')
    })

    // Re-push whenever any of the seven keys change. EventBus emits
    // `config:changed:<key>` from the configStore set() action.
    const aiKeys = [
      'ai.provider',
      'ai.model',
      'ai.apiKey',
      'ai.baseUrl',
      'ai.embedProvider',
      'ai.embedApiKey',
      'ai.embedBaseUrl',
    ]
    for (const key of aiKeys) {
      api.events.on(`config:changed:${key}`, () => {
        void pushUserConfig(api, readUserConfig())
      })
    }

    // ── BL-032 — Cmd+I overlay ────────────────────────────────────────────
    //
    // Wires the command-anywhere AI overlay. Uses the same kernel-side
    // `com.nexus.ai::stream_chat` channel as the chat view but mints a
    // distinct `cmdi-<uuid>` session id per activation so events don't
    // cross-contaminate the chat store. The overlay also lives in a
    // different slot (`overlay`) so it stacks over the workspace
    // independently of the AI chat panel.
    setCmdIApi(api)
    // BL-034 — register the same handle for the editor's inline ghost
    // completion. Held separately so future drains of one surface
    // (e.g. moving Cmd+I to a sandboxed plugin) don't unhook the
    // other.
    setGhostApi(api)
    // BL-036 phase 4 — same module-scoped handle pattern for the
    // margin-suggestions idle-trigger CM extension. The trigger
    // calls `requestPass(api, …)` which routes through
    // `com.nexus.ai::stream_chat`.
    setMarginApi(api)

    // Subscribe a SECOND time to the stream prefix specifically for the
    // overlay router. The chat-side `subscribeStream` already runs and
    // ignores `cmdi-` session ids (its store lookup misses every turn),
    // so the two subscriptions coexist without stepping on each other.
    // Both disposers are auto-swept on plugin unload via
    // `registry.trackSubscription` inside `api.kernel.on`. Awaited
    // below alongside the chat subscription so the first overlay submit
    // can't fire before the listener is live.
    const cmdISubPromise = api.kernel.on(
      'com.nexus.ai.stream_',
      (topic, payload) => {
        routeStreamEvent(topic, payload)
      },
    )

    api.commands.register(COMMAND_CMD_I_OPEN, async () => {
      await openCmdI()
    })
    api.commands.register(COMMAND_CMD_I_CLOSE, () => {
      useCmdIStore.getState().close()
    })

    // Mirror the overlay's `visible` flag into the context-key service
    // so the `escape` keybinding's `when` clause resolves correctly.
    api.context.set(
      CONTEXT_KEY_CMD_I_VISIBLE,
      useCmdIStore.getState().visible,
    )
    useCmdIStore.subscribe((state, prev) => {
      if (state.visible !== prev.visible) {
        api.context.set(CONTEXT_KEY_CMD_I_VISIBLE, state.visible)
      }
    })

    api.views.register(VIEW_ID_CMD_I_OVERLAY, {
      slot: 'overlay',
      // Sit just below the command palette (priority 10) so a user who
      // somehow opens both gets the palette on top — defensive only;
      // the palette closes on Cmd+I anyway through normal focus rules.
      priority: 20,
      component: CmdIOverlay,
    })

    // Register the first context contributor — the editor adapter
    // (current file + selection). The disposer is intentionally not
    // tracked: the AI plugin has no `deactivate`, the registry is a
    // module-scope singleton, and a hot plugin-reload at most leaves
    // one stale duplicate (which the registry tolerates). When the
    // PluginAPI grows a `trackSubscription` accessor (or this plugin
    // grows a `deactivate`), thread the disposer through there.
    registerEditorContextAdapter()

    // BL-035 — register the four built-in AI actions (summarize,
    // rewrite, translate, explain) against the shared
    // `aiActionRegistry`. Same disposer-not-tracked rationale as the
    // editor context adapter above: module-scope singleton, no
    // `deactivate` hook, hot-reload duplicates are tolerated by the
    // registry.
    registerBuiltinAiActions(api)

    // Fan out four awaits: subscription must be live before any submit
    // could fire (otherwise we'd miss the first chunks); the config
    // push lands the user's saved provider before hydrate reads it
    // back; sessions hydration is best-effort and non-blocking.
    await subscribeStream(api)
    await cmdISubPromise
    await pushUserConfig(api, readUserConfig())
    void hydrateConfig(api)
    void loadSessions(api)
  },

  // No deactivate — PluginRegistry.unregisterAll sweeps the kernel
  // subscription tracked by api.kernel.on (commit c4d31d3).
}
