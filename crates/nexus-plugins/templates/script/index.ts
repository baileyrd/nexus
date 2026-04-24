// {{plugin-name}} — sandboxed community plugin source.
//
// This TypeScript file is the plugin's author-facing source. The build step
// (`pnpm build`) bundles it together with the `bootstrapSandboxedPlugin`
// runtime from `@nexus/extension-api` into a single `index.js` that the
// Nexus shell loads inside a null-origin iframe sandbox.

import {
  bootstrapSandboxedPlugin,
  type SandboxedPlugin,
} from '@nexus/extension-api'

const plugin: SandboxedPlugin = {
  async activate(ctx) {
    // Commands registered here show up in the command palette and can be
    // invoked from menus, keybindings, or panel buttons.
    ctx.commands.register('{{plugin-id}}.hello', async () => {
      await ctx.notifications.show({
        message: 'Hello from {{plugin-name}}!',
        type: 'info',
        duration: 3000,
      })
    })

    // Panel views use the declarative `PanelNode` tree — the host renders
    // them; React components cannot cross the sandbox boundary.
    ctx.views.registerPanel('{{plugin-id}}.panel', () => ({
      type: 'vstack',
      gap: 8,
      children: [
        { type: 'heading', value: '{{plugin-name}}', level: 2 },
        { type: 'text', value: '{{description}}' },
        {
          type: 'button',
          label: 'Say hello',
          commandId: '{{plugin-id}}.hello',
        },
      ],
    }))
  },
}

bootstrapSandboxedPlugin(plugin)

export default plugin
