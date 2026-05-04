import type { Plugin, PluginAPI } from '../../../types/plugin'

const PLUGIN_ID = 'com.nexus.templates'
const HANDLER_LIST = 'list'
const HANDLER_APPLY = 'apply'

const COMMAND_NEW = 'nexus.templates.new'
const COMMAND_LIST = 'nexus.templates.list'

interface TemplateParameter {
  name: string
  type?: string
  default?: string | null
  required?: boolean
  description?: string | null
}

interface TemplateMeta {
  name: string
  description: string | null
  target_path: string | null
  parameters: TemplateParameter[]
}

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
      ],
    },
  },

  async activate(api: PluginAPI) {
    const fetchList = async (): Promise<TemplateMeta[]> => {
      const raw = await api.kernel.invoke<unknown>(PLUGIN_ID, HANDLER_LIST, {})
      if (!Array.isArray(raw)) return []
      return raw as TemplateMeta[]
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

      let list: TemplateMeta[]
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
