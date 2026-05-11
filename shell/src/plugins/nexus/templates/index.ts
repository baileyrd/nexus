import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { TemplatesView } from './TemplatesView'
import { templatesPaneViewCreator } from './TemplatesPaneView'
import { useTemplatesStore, type TemplateEntry } from './templatesStore'

const PLUGIN_ID = 'com.nexus.templates'
const HANDLER_LIST = 'list'
const HANDLER_APPLY = 'apply'

const COMMAND_NEW = 'nexus.templates.new'
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

/**
 * Page-template commands. Wraps `com.nexus.templates` IPC behind palette
 * commands.
 *
 * Two commands today:
 *   - "Templates: List available" — drops the names into a notification
 *     so the user can see what's installed before invoking "New".
 *   - "Templates: New from template…" — prompts for a name, prompts for
 *     each declared parameter in order, then applies.
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
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar'],
    contributes: {
      commands: [
        {
          id: COMMAND_NEW,
          title: 'New from template…',
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

    api.commands.register(COMMAND_NEW, async () => {
      if (!(await api.kernel.available())) {
        api.notifications.show({
          message: 'Open a workspace to apply templates.',
          type: 'warning',
        })
        return
      }

      let list: TemplateEntry[]
      try {
        list = await fetchList()
      } catch (err) {
        api.notifications.show({
          message: `Failed to list templates: ${err instanceof Error ? err.message : String(err)}`,
          type: 'error',
        })
        return
      }
      if (list.length === 0) {
        api.notifications.show({
          message: 'No templates available.',
          type: 'warning',
        })
        return
      }

      const names = list.map((t) => t.name).join(', ')
      const picked = await api.input.prompt(
        `Template name (one of: ${names})`,
        list[0]?.name ?? '',
      )
      if (!picked) return
      const tpl = list.find((t) => t.name === picked.trim())
      if (!tpl) {
        api.notifications.show({
          message: `Unknown template: ${picked}`,
          type: 'error',
        })
        return
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
        if (value === null) return // cancel halts the whole flow
        if (value.trim().length > 0) {
          args[param.name] = value
        } else if (param.required) {
          api.notifications.show({
            message: `'${param.name}' is required — aborting.`,
            type: 'error',
          })
          return
        }
      }

      try {
        const result = await api.kernel.invoke<ApplyResult>(
          PLUGIN_ID,
          HANDLER_APPLY,
          { name: tpl.name, args },
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
  },
}
