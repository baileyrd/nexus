import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useEditorStore } from './editorStore'

export { useEditorStore } from './editorStore'
export type { EditorTab } from './editorStore'

export const editorAreaPlugin: Plugin = {
  manifest: {
    id: 'core.editor-area',
    name: 'Editor Area',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: 'editor.closeTab',     title: 'Close Tab',         category: 'Editor' },
        { id: 'editor.closeAllTabs', title: 'Close All Tabs',    category: 'Editor' },
        { id: 'editor.nextTab',      title: 'Next Tab',          category: 'Editor' },
        { id: 'editor.previousTab',  title: 'Previous Tab',      category: 'Editor' },
        { id: 'editor.pinTab',       title: 'Pin / Unpin Tab',   category: 'Editor' },
      ],
      keybindings: [
        { command: 'editor.closeTab',    key: 'ctrl+w',         mac: 'cmd+w',         when: 'editorFocus' },
        { command: 'editor.nextTab',     key: 'ctrl+tab',                             when: 'editorFocus' },
        { command: 'editor.previousTab', key: 'ctrl+shift+tab',                       when: 'editorFocus' },
      ],
    },
  },
  activate(api: PluginAPI) {
    // Phase 7: legacy slot:'editorArea' registration removed.
    // EditorAreaView is no longer mounted via SlotRegistry.

    api.commands.register('editor.closeTab', () => {
      const { activeTabId, closeTab } = useEditorStore.getState()
      if (activeTabId) closeTab(activeTabId)
    })
    api.commands.register('editor.closeAllTabs', () => {
      useEditorStore.getState().tabs.forEach(t => useEditorStore.getState().closeTab(t.id))
    })
    api.commands.register('editor.nextTab', () => {
      const { tabs, activeTabId, setActiveTab } = useEditorStore.getState()
      const idx = tabs.findIndex(t => t.id === activeTabId)
      const next = tabs[(idx + 1) % tabs.length]
      if (next) setActiveTab(next.id)
    })
    api.commands.register('editor.previousTab', () => {
      const { tabs, activeTabId, setActiveTab } = useEditorStore.getState()
      const idx = tabs.findIndex(t => t.id === activeTabId)
      const prev = tabs[(idx - 1 + tabs.length) % tabs.length]
      if (prev) setActiveTab(prev.id)
    })
    api.commands.register('editor.pinTab', () => {
      const { activeTabId, pinTab } = useEditorStore.getState()
      if (activeTabId) pinTab(activeTabId)
    })

    useEditorStore.subscribe(state => {
      api.context.set('editorFocus', state.activeTabId !== null)
      api.context.set('editorHasTabs', state.tabs.length > 0)
    })
  },
}
