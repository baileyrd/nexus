import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ThemePicker } from './ThemePicker'
import { useThemePickerStore } from './themePickerStore'
import { setPickerApi } from './pickerRuntime'

const COMMAND_OPEN       = 'nexus.themePicker.open'
const COMMAND_CLOSE      = 'nexus.themePicker.close'
const VIEW_ID            = 'nexus.themePicker.overlay'
const CONTEXT_KEY        = 'nexus.themePicker.visible'
const ACTIVITY_ITEM_ID   = 'nexus.themePicker.activityBarItem'
const ACTIVITY_VIEW_SLOT = 'nexus.themePicker.activityView'

export const themePickerPlugin: Plugin = {
  manifest: {
    id: 'nexus.themePicker',
    name: 'Theme Picker',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.activityBar'],
    contributes: {
      commands: [
        {
          id: COMMAND_OPEN,
          title: 'Open Theme Picker',
          category: 'Appearance',
        },
        {
          id: COMMAND_CLOSE,
          title: 'Close Theme Picker',
          category: 'Appearance',
        },
      ],
      keybindings: [
        { command: COMMAND_OPEN, key: 'ctrl+shift+t', mac: 'cmd+shift+t' },
        { command: COMMAND_CLOSE, key: 'escape', when: CONTEXT_KEY },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY,
          description: 'True while the theme picker overlay is open.',
          type: 'boolean',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    setPickerApi(api)

    api.commands.register(COMMAND_OPEN, () => {
      useThemePickerStore.getState().open()
    })

    api.commands.register(COMMAND_CLOSE, () => {
      useThemePickerStore.getState().close()
    })

    // Mirror store visibility into the context-key service so the
    // `escape` keybinding `when` clause works for other plugins too.
    api.context.set(CONTEXT_KEY, useThemePickerStore.getState().visible)
    useThemePickerStore.subscribe((state, prev) => {
      if (state.visible !== prev.visible) {
        api.context.set(CONTEXT_KEY, state.visible)
      }
    })

    api.views.register(VIEW_ID, {
      slot: 'overlay',
      component: ThemePicker,
      priority: 15,
    })

    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconName: 'sliders',
      title: 'Themes',
      viewId: ACTIVITY_VIEW_SLOT,
      priority: 95,
      placement: 'bottom',
      command: COMMAND_OPEN,
    })
  },
}
