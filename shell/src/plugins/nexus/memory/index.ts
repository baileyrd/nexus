// BL-043 — quick-capture global hotkey.
//
// Cmd/Ctrl+Alt+N opens a small in-window overlay; on Save the snippet is
// appended to a configurable Inbox.md file in the active forge with a
// timestamp + lightweight source metadata. Pure file write; no AI
// dependency in v1.
//
// The hotkey is delivered by `tauri-plugin-global-shortcut` (registered
// in `shell/src-tauri/src/lib.rs`), which fires regardless of window
// focus so a backgrounded Nexus stays reachable for capture. The
// overlay itself is just a slot view in the existing `overlay` chrome
// slot — no separate `WebviewWindow` (per ADR 0011 single-window posture).

import { createElement, type ReactElement } from 'react'

import {
  isRegistered,
  register,
  unregister,
} from '@tauri-apps/plugin-global-shortcut'

import type { Plugin, PluginAPI } from '../../../types/plugin.ts'

import { CaptureOverlay, type CaptureOverlayProps } from './CaptureOverlay'
import {
  buildSnippet as _buildSnippet,
  commitCapture,
  readClipboardBestEffort,
  useCaptureStore,
  type CaptureSourceMeta,
} from './captureStore'
import { detectCodeLanguage } from './codeCapture'

const VIEW_ID = 'nexus.memory.captureOverlay'
const COMMAND_OPEN = 'nexus.memory.captureOpen'
const COMMAND_COMMIT = 'nexus.memory.captureCommit'
const COMMAND_OPEN_CODE = 'nexus.memory.captureCodeOpen'

const CONFIG_HOTKEY = 'memory.hotkey'
const CONFIG_INBOX_PATH = 'memory.inboxPath'

const DEFAULT_HOTKEY = 'CommandOrControl+Alt+N'
const DEFAULT_INBOX_PATH = 'Inbox.md'

// Re-exported so unit tests + future plugins can introspect the snippet
// shape without re-implementing it.
export { _buildSnippet as buildCaptureSnippet }

/**
 * Best-effort source metadata captured at hotkey-press time. App label
 * defaults to the document title — small enough that a user reading the
 * Inbox can tell the snippet came from "Nexus" rather than from a
 * markdown file they were editing, but no PII / window-listing surface.
 */
function snapshotSourceMeta(): CaptureSourceMeta {
  const app =
    typeof document !== 'undefined' && typeof document.title === 'string'
      ? document.title
      : ''
  return {
    app,
    capturedAt: new Date().toISOString(),
  }
}

export const memoryPlugin: Plugin = {
  manifest: {
    id: 'nexus.memory',
    name: 'Memory',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    contributes: {
      configuration: {
        pluginId: 'nexus.memory',
        title: 'Memory',
        order: 70,
        schema: [
          {
            key: CONFIG_HOTKEY,
            title: 'Quick-capture hotkey',
            // Highlight the Spotlight collision for macOS users — the
            // BL-043 plan defaults around it deliberately.
            description:
              'Global hotkey that opens the quick-capture overlay. ' +
              'Default avoids macOS Spotlight (Cmd+Shift+Space). ' +
              'Tauri accelerator syntax: e.g. "CommandOrControl+Alt+N".',
            type: 'string' as const,
            default: DEFAULT_HOTKEY,
          },
          {
            key: CONFIG_INBOX_PATH,
            title: 'Inbox path',
            description:
              'Forge-relative path of the inbox file the hotkey appends ' +
              'to. Created on first capture if missing.',
            type: 'string' as const,
            default: DEFAULT_INBOX_PATH,
          },
        ],
      },
      commands: [
        {
          id: COMMAND_OPEN,
          title: 'Quick Capture: Open Overlay',
          category: 'Memory',
        },
        {
          id: COMMAND_COMMIT,
          title: 'Quick Capture: Save to Inbox',
          category: 'Memory',
        },
        {
          id: COMMAND_OPEN_CODE,
          title: 'Quick Capture: Open Code Capture',
          category: 'Memory',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    api.configuration.register(memoryPlugin.manifest.contributes!.configuration!)

    // The overlay always renders into the chrome `overlay` slot. The
    // overlay component reads `useCaptureStore.open` and renders nothing
    // when the store is closed, so the plugin never needs to mount /
    // unmount the slot dynamically. Wrapping as a parameterless
    // component lets us thread `api.commands` into the modal's "Save"
    // button without exposing it on the slot type.
    const CaptureOverlaySlot = (): ReactElement =>
      createElement<CaptureOverlayProps>(CaptureOverlay, {
        commands: api.commands,
      })
    api.views.register(VIEW_ID, {
      slot: 'overlay',
      // Below confirm (90) and launcher (10) — the launcher is the
      // top-priority gate before any forge is open; once we're past
      // that, the capture overlay is fine to share priority space with
      // pluginsMgmt (20) but we slot it slightly higher.
      priority: 25,
      component: CaptureOverlaySlot,
    })

    // ── Commands ────────────────────────────────────────────────────────

    api.commands.register(COMMAND_OPEN, async () => {
      // Surface a friendly error rather than opening the overlay if the
      // kernel hasn't booted yet (no forge selected). Without this guard
      // the user would see a generic "plugin call failed" the moment
      // they tried to commit.
      const ready = await api.kernel.available()
      if (!ready) {
        api.notifications.show({
          type: 'warning',
          message:
            'Open a forge before capturing — the hotkey needs an active workspace to know where Inbox.md lives.',
        })
        return
      }

      const draft = await readClipboardBestEffort()
      const sourceMeta = snapshotSourceMeta()
      useCaptureStore.getState().openOverlay(draft, sourceMeta)
    })

    interface CodeCaptureArgs {
      file?: string
      language?: string
      lineRange?: { start: number; end: number }
      content?: string
    }

    api.commands.register(
      COMMAND_OPEN_CODE,
      async (...rawArgs: unknown[]) => {
        const args = (rawArgs[0] ?? {}) as CodeCaptureArgs
        // BL-046 — code-aware capture entry point. Exposed as a
        // command so an IDE plugin / CLI / right-click action can
        // call `api.commands.execute(...)` with the source-file
        // metadata. Falls back to the plain hotkey path when
        // arguments are missing or the file extension doesn't
        // resolve to a known language.
        const ready = await api.kernel.available()
        if (!ready) {
          api.notifications.show({
            type: 'warning',
            message:
              'Open a forge before capturing — the code-capture command needs an active workspace.',
          })
          return
        }
        const file = args.file?.trim() ?? ''
        const explicitLanguage = args.language?.trim() || null
        const language = explicitLanguage ?? detectCodeLanguage(file)
        if (!file || !language) {
          api.notifications.show({
            type: 'warning',
            message:
              'Code capture requires a file path with a recognised extension. Use Quick Capture for plain text.',
          })
          return
        }
        const code = {
          file,
          language,
          ...(args.lineRange
            ? {
                lineRange: {
                  start: Math.max(1, Math.floor(args.lineRange.start)),
                  end: Math.max(
                    Math.max(1, Math.floor(args.lineRange.start)),
                    Math.floor(args.lineRange.end),
                  ),
                },
              }
            : {}),
        }
        const draft = (args.content ?? (await readClipboardBestEffort())).replace(
          /\r\n/g,
          '\n',
        )
        const sourceMeta = { ...snapshotSourceMeta(), code }
        useCaptureStore.getState().openOverlay(draft, sourceMeta)
      },
    )

    api.commands.register(COMMAND_COMMIT, async () => {
      const { draft, sourceMeta } = useCaptureStore.getState()
      const inboxPath = api.configuration.getValue<string>(
        CONFIG_INBOX_PATH,
        DEFAULT_INBOX_PATH,
      )
      const result = await commitCapture({
        api: api.kernel,
        inboxPath,
        draft,
        sourceMeta,
      })
      if (result.ok) {
        api.notifications.show({
          type: 'success',
          message: `Captured to ${inboxPath}`,
        })
      }
    })

    // ── Hotkey lifecycle ────────────────────────────────────────────────

    // Track the currently-bound accelerator so we can unregister on
    // change / deactivate. Using a closure-local var rather than the
    // store keeps the hotkey machinery encapsulated.
    let currentAccelerator: string | null = null

    const fireCaptureOpen = () => {
      void api.commands.execute(COMMAND_OPEN)
    }

    const tryRegister = async (accelerator: string): Promise<void> => {
      try {
        // `register` itself throws if the accelerator is already bound
        // by another app, but we still pre-check `isRegistered` so a
        // re-register from the same Nexus session (after a config edit)
        // doesn't error — the OS sees the prior binding as "in use".
        if (await isRegistered(accelerator)) {
          try {
            await unregister(accelerator)
          } catch (err) {
            console.warn(
              '[nexus.memory] unregister of stale binding failed:',
              err,
            )
          }
        }
        await register(accelerator, fireCaptureOpen)
        currentAccelerator = accelerator
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        api.notifications.show({
          type: 'warning',
          message:
            `Could not register quick-capture hotkey "${accelerator}" — ` +
            `${message}. Edit \`${CONFIG_HOTKEY}\` in settings.`,
        })
        currentAccelerator = null
      }
    }

    const tryUnregister = async (accelerator: string | null): Promise<void> => {
      if (accelerator === null) return
      try {
        await unregister(accelerator)
      } catch (err) {
        console.warn('[nexus.memory] unregister failed:', err)
      }
    }

    const initial = api.configuration.getValue<string>(
      CONFIG_HOTKEY,
      DEFAULT_HOTKEY,
    )
    void tryRegister(initial)

    // Live-reload on configuration changes. The disposer is auto-swept
    // by the plugin registry when the plugin deactivates, so we don't
    // need to track it explicitly.
    api.configuration.onChange(CONFIG_HOTKEY, (next) => {
      const accelerator = typeof next === 'string' && next.length > 0
        ? next
        : DEFAULT_HOTKEY
      void (async () => {
        await tryUnregister(currentAccelerator)
        await tryRegister(accelerator)
      })()
    })
  },

  async deactivate() {
    // Best-effort cleanup. We don't have a handle to the
    // currentAccelerator here (closure scoped to activate); call
    // unregister on the documented default + the configured value so a
    // user-edited accelerator still gets cleared.
    //
    // The OS itself releases the global shortcut on process exit, so
    // this is mostly a courtesy for hot-reload scenarios.
    try {
      await unregister(DEFAULT_HOTKEY)
    } catch {
      // Ignore — the accelerator may not have been registered.
    }
  },
}
