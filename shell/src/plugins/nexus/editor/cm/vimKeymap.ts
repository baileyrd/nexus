import { StateField, StateEffect, type Extension } from '@codemirror/state'
import type { EditorView } from '@codemirror/view'
import { Vim, vim } from '@replit/codemirror-vim'

/**
 * BL-070: optional Vim keybinding layer for the Nexus markdown editor.
 *
 * `vim()` from `@replit/codemirror-vim` installs Normal/Insert/Visual
 * modes plus the standard motion / operator vocabulary; this module
 * adds the Nexus-specific glue that keeps the modal layer integrated
 * with the kernel session:
 *
 *   - `:w` / `:wq` / `:x` route through the editor's
 *     `saveSession(relpath)` IPC instead of CodeMirror-local state, so
 *     the on-disk markdown matches what every other consumer (AI, MCP,
 *     storage watcher) reads.
 *   - `:q` closes the active tab via the host-supplied `onClose`.
 *
 * The slash-command collision noted in BL-070's DoD resolves naturally:
 * `slashCommand.ts` only fires when `/` is typed at the start of an
 * empty paragraph, which only happens in Vim Insert mode — Normal-mode
 * `/` lands at vim's search layer before the slash extension sees it.
 */
export interface VimKeymapOptions {
  /** Forge-relative path of the tab the keymap is mounted on. */
  relpath: string
  /** `:w` / `:wq` / `:x` route here. */
  onSave: () => void
  /** `:q` / `:wq` / `:x` route here. */
  onClose: () => void
}

interface VimContext {
  relpath: string
  onSave: () => void
  onClose: () => void
}

const setVimContext = StateEffect.define<VimContext>()

/**
 * Per-view storage for the Vim ex-command handlers. `Vim.defineEx`
 * is process-global — the ex commands themselves can't be tied to a
 * specific view — so we stash the per-tab callbacks here and the
 * handlers read them off `view.state.field(vimContextField)` at
 * dispatch time.
 */
const vimContextField = StateField.define<VimContext | null>({
  create: () => null,
  update(value, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setVimContext)) return effect.value
    }
    return value
  },
})

let exCommandsRegistered = false

function registerExCommandsOnce(): void {
  if (exCommandsRegistered) return
  exCommandsRegistered = true

  // `Vim.defineEx(name, prefix, fn)` accepts a unique prefix that lets
  // the ex parser dispatch on shorter forms (`:w` matches `:write`
  // when prefix is `'w'`). We register the canonical name; the ex
  // parser handles partial-prefix matching on its own once a command
  // is defined.
  const handlers: Array<[string, string, (ctx: VimContext) => void]> = [
    ['write', 'w', (ctx) => ctx.onSave()],
    ['quit', 'q', (ctx) => ctx.onClose()],
    [
      'wq',
      'wq',
      (ctx) => {
        ctx.onSave()
        ctx.onClose()
      },
    ],
    [
      'x',
      'x',
      (ctx) => {
        ctx.onSave()
        ctx.onClose()
      },
    ],
  ]

  for (const [name, prefix, run] of handlers) {
    Vim.defineEx(name, prefix, (cm: { cm6: EditorView }) => {
      const view = cm.cm6
      const ctx = view.state.field(vimContextField, false) ?? null
      if (!ctx) return
      run(ctx)
    })
  }
}

/**
 * Build the Vim keymap extension stack for a single tab. Idempotent at
 * the global level (ex commands are registered exactly once per
 * process); per-view state is reinstalled on every mount because the
 * tab's `relpath` and host callbacks change between mounts.
 */
export function vimKeymapExt(opts: VimKeymapOptions): Extension {
  registerExCommandsOnce()
  const { relpath, onSave, onClose } = opts
  const ctx: VimContext = { relpath, onSave, onClose }
  return [
    vim({ status: true }),
    vimContextField.init(() => ctx),
  ]
}
