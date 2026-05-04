import type { Plugin, PluginAPI } from '../../../types/plugin'

const PLUGIN_ID = 'com.nexus.formats'
const HANDLER_IMPORT = 'import_notion'
const HANDLER_EXPORT = 'export_notion'

const COMMAND_IMPORT = 'nexus.notion.import'
const COMMAND_EXPORT = 'nexus.notion.export'

interface ImportReport {
  pages_written: number
  bases_written: number
  attachments_copied: number
  warnings: string[]
  dest: string
}

interface ExportReport {
  pages_written: number
  databases_written: number
  attachments_copied: number
  warnings: string[]
  dest: string
}

/**
 * Notion import/export commands. Wraps `com.nexus.formats` IPC behind
 * palette commands that prompt the user for paths and report the
 * conversion summary as a notification.
 *
 * The import/export logic itself lives in the `nexus-formats` Rust
 * crate (see `crates/nexus-formats/src/notion/`); this plugin is a thin
 * UI shim around it.
 */
export const notionPlugin: Plugin = {
  manifest: {
    id: 'nexus.notion',
    name: 'Notion',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        {
          id: COMMAND_IMPORT,
          title: 'Import from Notion zip…',
          category: 'Notion',
        },
        {
          id: COMMAND_EXPORT,
          title: 'Export to Notion folder…',
          category: 'Notion',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    api.commands.register(COMMAND_IMPORT, async () => {
      if (!(await api.kernel.available())) {
        api.notifications.show({
          message: 'Open a workspace before importing.',
          type: 'warning',
        })
        return
      }

      const source = await api.input.prompt(
        'Path to a Notion zip export',
        '/path/to/Export-…zip',
      )
      if (!source) return

      const dest = await api.input.prompt(
        'Destination subfolder (forge-relative). Leave blank for "Imported from Notion".',
        'Imported from Notion',
      )
      if (dest === null) return // explicit cancel

      try {
        const result = await api.kernel.invoke<ImportReport>(
          PLUGIN_ID,
          HANDLER_IMPORT,
          {
            source,
            ...(dest.trim().length > 0 ? { dest } : {}),
          },
        )
        const note =
          `Imported ${result.pages_written} pages, ` +
          `${result.bases_written} databases, ` +
          `${result.attachments_copied} attachments → ${result.dest}` +
          (result.warnings.length > 0
            ? ` (${result.warnings.length} warning(s))`
            : '')
        api.notifications.show({
          message: note,
          type: result.warnings.length > 0 ? 'warning' : 'success',
          duration: 6000,
        })
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        api.notifications.show({
          message: `Notion import failed: ${message}`,
          type: 'error',
          duration: 8000,
        })
      }
    })

    api.commands.register(COMMAND_EXPORT, async () => {
      if (!(await api.kernel.available())) {
        api.notifications.show({
          message: 'Open a workspace before exporting.',
          type: 'warning',
        })
        return
      }

      const source = await api.input.prompt(
        'Forge-relative folder to export. Leave blank to export the whole forge.',
        '',
      )
      if (source === null) return

      const dest = await api.input.prompt(
        'Output directory (absolute path). Will be created if missing.',
        '/tmp/notion-export',
      )
      if (!dest) return

      try {
        const result = await api.kernel.invoke<ExportReport>(
          PLUGIN_ID,
          HANDLER_EXPORT,
          {
            ...(source.trim().length > 0 ? { source } : {}),
            dest,
          },
        )
        const note =
          `Exported ${result.pages_written} pages, ` +
          `${result.databases_written} databases, ` +
          `${result.attachments_copied} attachments → ${result.dest}` +
          (result.warnings.length > 0
            ? ` (${result.warnings.length} warning(s))`
            : '')
        api.notifications.show({
          message: note,
          type: result.warnings.length > 0 ? 'warning' : 'success',
          duration: 6000,
        })
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        api.notifications.show({
          message: `Notion export failed: ${message}`,
          type: 'error',
          duration: 8000,
        })
      }
    })
  },
}
