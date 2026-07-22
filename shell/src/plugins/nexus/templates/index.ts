import { createElement } from 'react'
import { EditorSelection } from '@codemirror/state'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { getActiveCmView } from '../editor/runtime'
import { TemplatesView } from './TemplatesView'
import { templatesPaneViewCreator } from './TemplatesPaneView'
import { useTemplatesStore, type TemplateEntry } from './templatesStore'

const PLUGIN_ID = 'com.nexus.templates'
const HANDLER_LIST = 'list'
const HANDLER_RENDER = 'render'
const HANDLER_APPLY = 'apply'

const COMMAND_NEW = 'nexus.templates.new'
const COMMAND_INSERT = 'nexus.templates.insertAtCursor'
const COMMAND_LIST = 'nexus.templates.list'
const COMMAND_SHOW = 'nexus.templates.show'
const COMMAND_REFRESH = 'nexus.templates.refresh'
const VIEW_ID = 'nexus.templates.view'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

interface ApplyResult {
  name: string
  path: string
  absolute_path: string
}

interface RenderResult {
  name: string
  target_path: string
  body: string
}

/**
 * Page-template commands. Wraps `com.nexus.templates` IPC behind palette
 * commands.
 *
 *   - "Templates: List available" — drops the names into a notification
 *     so the user can see what's installed before invoking "New".
 *   - "Templates: New from template…" — prompts for a name, prompts for
 *     each declared parameter in order, then applies (writes a new file).
 *   - "Templates: Insert template at cursor…" (#367 / C14) — same prompt
 *     flow, but renders (dry run) and inserts the body into the active
 *     editor at the cursor instead of creating a file.
 *
 * The template engine itself lives in the `nexus-templates` Rust crate;
 * this plugin is a thin UI shim around it.
 */
export const templatesPlugin: Plugin = {
  manifest: {
    id: 'nexus.templates',
    name: 'Templates',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    // Dispatches the `nexus.files.openByPath` command to reveal newly
    // created notes — files plugin must be loaded.
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.files', 'nexus.editor'],
    contributes: {
      commands: [
        {
          id: COMMAND_NEW,
          title: 'New from template…',
          category: 'Templates',
        },
        {
          id: COMMAND_INSERT,
          title: 'Insert template at cursor…',
          category: 'Templates',
        },
        {
          id: COMMAND_LIST,
          title: 'List available templates',
          category: 'Templates',
        },
        {
          id: COMMAND_SHOW,
          title: 'Show Templates panel',
          category: 'Templates',
        },
        {
          id: COMMAND_REFRESH,
          title: 'Refresh templates',
          category: 'Templates',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    const fetchList = async (): Promise<TemplateEntry[]> => {
      const raw = await api.kernel.invoke<unknown>(PLUGIN_ID, HANDLER_LIST, {})
      if (!Array.isArray(raw)) return []
      return raw as TemplateEntry[]
    }

    const refresh = async (): Promise<void> => {
      const store = useTemplatesStore.getState()
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (!available) {
        store.setLoading(false)
        store.setLoadError('Open a workspace to load templates.')
        store.setTemplates([])
        return
      }
      store.setLoading(true)
      store.setLoadError(null)
      try {
        const list = await fetchList()
        useTemplatesStore.getState().setTemplates(list)
        useTemplatesStore.getState().setLoading(false)
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useTemplatesStore.getState().setLoadError(message)
        useTemplatesStore.getState().setTemplates([])
        useTemplatesStore.getState().setLoading(false)
      }
    }

    const renderTemplatesView = () =>
      createElement(TemplatesView, {
        kernel: api.kernel,
        onRefresh: () => void refresh(),
        notify: (message, type = 'info') =>
          api.notifications.show({ message, type, duration: 5000 }),
        openFile: (path: string) =>
          void api.commands.execute('nexus.files.openByPath', path).catch(() => {
            // openByPath may not exist in every shell build — silent fall-through.
          }),
      })

    api.viewRegistry.register('templates', templatesPaneViewCreator(renderTemplatesView))

    api.activityBar.addItem({
      id: 'nexus.templates.activityItem',
      icon: '',
      iconName: 'template',
      title: 'Templates',
      viewId: VIEW_ID,
      priority: 45,
      command: COMMAND_SHOW,
    })

    api.commands.register(COMMAND_REFRESH, () => {
      void refresh()
    })
    api.commands.register(COMMAND_SHOW, async () => {
      const leaf = await workspace.ensureLeafOfType('templates', 'main')
      workspace.revealLeaf(leaf)
    })

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void refresh()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useTemplatesStore.getState().reset()
    })
    if (await api.kernel.available()) {
      void refresh()
    }

    api.commands.register(COMMAND_LIST, async () => {
      if (!(await api.kernel.available())) {
        api.notifications.show({
          message: 'Open a workspace to load templates.',
          type: 'warning',
        })
        return
      }
      try {
        const list = await fetchList()
        const summary = list
          .map((t) => `• ${t.name}${t.description ? ` — ${t.description}` : ''}`)
          .join('\n')
        api.notifications.show({
          message: `${list.length} template(s):\n${summary}`,
          type: 'info',
          duration: 12000,
        })
      } catch (err) {
        api.notifications.show({
          message: `Failed to list templates: ${err instanceof Error ? err.message : String(err)}`,
          type: 'error',
        })
      }
    })

    /**
     * Shared "pick a template, then prompt for its declared params in
     * order" flow used by both COMMAND_NEW (apply → new file) and
     * COMMAND_INSERT (render → insert at cursor). Returns `null` on
     * any cancel / validation failure — the notification for that case
     * is already shown by the time this returns, so callers just bail.
     */
    const promptForTemplateAndArgs = async (
      list: TemplateEntry[],
    ): Promise<{ tpl: TemplateEntry; args: Record<string, string> } | null> => {
      const names = list.map((t) => t.name).join(', ')
      const picked = await api.input.prompt(
        `Template name (one of: ${names})`,
        list[0]?.name ?? '',
      )
      if (!picked) return null
      const tpl = list.find((t) => t.name === picked.trim())
      if (!tpl) {
        api.notifications.show({
          message: `Unknown template: ${picked}`,
          type: 'error',
        })
        return null
      }

      // Prompt for each declared parameter in order. Required parameters
      // must get a value; optional ones can be left blank to fall through
      // to the default.
      const args: Record<string, string> = {}
      for (const param of tpl.parameters ?? []) {
        const label =
          `[${tpl.name}] ${param.name}` +
          (param.required ? ' (required)' : '') +
          (param.description ? ` — ${param.description}` : '')
        const placeholder = param.default ?? ''
        const value = await api.input.prompt(label, placeholder)
        if (value === null) return null // cancel halts the whole flow
        if (value.trim().length > 0) {
          args[param.name] = value
        } else if (param.required) {
          api.notifications.show({
            message: `'${param.name}' is required — aborting.`,
            type: 'error',
          })
          return null
        }
      }
      return { tpl, args }
    }

    /** Shared "kernel available + non-empty list" precondition for both
     *  template commands. Returns `null` (having already notified) on
     *  failure. */
    const fetchListOrNotify = async (): Promise<TemplateEntry[] | null> => {
      if (!(await api.kernel.available())) {
        api.notifications.show({
          message: 'Open a workspace to apply templates.',
          type: 'warning',
        })
        return null
      }
      let list: TemplateEntry[]
      try {
        list = await fetchList()
      } catch (err) {
        api.notifications.show({
          message: `Failed to list templates: ${err instanceof Error ? err.message : String(err)}`,
          type: 'error',
        })
        return null
      }
      if (list.length === 0) {
        api.notifications.show({
          message: 'No templates available.',
          type: 'warning',
        })
        return null
      }
      return list
    }

    api.commands.register(COMMAND_NEW, async () => {
      const list = await fetchListOrNotify()
      if (!list) return
      const picked = await promptForTemplateAndArgs(list)
      if (!picked) return

      try {
        const result = await api.kernel.invoke<ApplyResult>(
          PLUGIN_ID,
          HANDLER_APPLY,
          { name: picked.tpl.name, args: picked.args },
        )
        api.notifications.show({
          message: `Created ${result.path}`,
          type: 'success',
          duration: 5000,
          actions: [
            {
              label: 'Open',
              command: `nexus.files.openByPath:${result.path}`,
            },
          ],
        })
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        api.notifications.show({
          message: `Apply failed: ${message}`,
          type: 'error',
          duration: 8000,
        })
      }
    })

    // #367 (C14) — the Templater-style "insert at cursor" workflow:
    // renders the template (com.nexus.templates::render — a dry run,
    // no file write) and inserts the body into the active editor at
    // the current selection via a plain CM6 dispatch. Bridge-eligible
    // tabs pick this up through the same `transactionBridge`
    // `updateListener` every other in-editor mutation goes through
    // (block handles, attachment paste, etc.) — no special-casing
    // needed here.
    api.commands.register(COMMAND_INSERT, async () => {
      const view = getActiveCmView()
      if (!view) {
        api.notifications.show({
          message: 'Insert template requires an active editor tab.',
          type: 'warning',
        })
        return
      }

      const list = await fetchListOrNotify()
      if (!list) return
      const picked = await promptForTemplateAndArgs(list)
      if (!picked) return

      try {
        const result = await api.kernel.invoke<RenderResult>(
          PLUGIN_ID,
          HANDLER_RENDER,
          { name: picked.tpl.name, args: picked.args },
        )
        // Re-check the view is still mounted — the two prompt() calls
        // above are async and the user could have switched/closed the
        // tab meanwhile.
        const current = getActiveCmView()
        if (!current) {
          api.notifications.show({
            message: 'Insert template: the editor tab closed before the template finished rendering.',
            type: 'warning',
          })
          return
        }
        const sel = current.state.selection.main
        current.dispatch({
          changes: { from: sel.from, to: sel.to, insert: result.body },
          selection: EditorSelection.cursor(sel.from + result.body.length),
          scrollIntoView: true,
        })
        current.focus()
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        api.notifications.show({
          message: `Insert template failed: ${message}`,
          type: 'error',
          duration: 8000,
        })
      }
    })
  },
}
