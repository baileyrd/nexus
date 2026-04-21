import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useLayoutStore } from '../../../stores/layoutStore'
import { SidebarHost } from './SidebarHost'
import { useSidebarSplitStore } from './sidebarSplitStore'

const EVENT_SHOW = 'sidebar:showView'
const EVENT_HIDE = 'sidebar:hide'

export const sidebarPlugin: Plugin = {
  manifest: {
    id: 'nexus.sidebar',
    name: 'Sidebar',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    const layout = useLayoutStore.getState()

    // Start with no active view. persist middleware may have retained
    // a stale id from a previous run (e.g. 'fileExplorer' from the
    // template's default) — wipe it so we don't point the host at a
    // view that no plugin contributes.
    layout.setActiveSidebarView('')
    if (layout.sidebar.visible) {
      useLayoutStore.setState((s) => ({ sidebar: { ...s.sidebar, visible: false } }))
    }

    api.events.on<{ viewId: string }>(EVENT_SHOW, ({ viewId }) => {
      // Obsidian-faithful reveal: find an existing leaf of this type and
      // activate it, else create a new one. The legacy layoutStore
      // activeView field is kept as a mirror so the activity bar's
      // "which icon is highlighted" logic still lines up.
      useSidebarSplitStore.getState().revealLeaf(viewId)
      useLayoutStore.getState().setActiveSidebarView(viewId)
      useLayoutStore.setState((s) => ({ sidebar: { ...s.sidebar, visible: true } }))
    })

    api.events.on(EVENT_HIDE, () => {
      useLayoutStore.setState((s) => ({ sidebar: { ...s.sidebar, visible: false } }))
    })

    api.views.register('nexus.sidebar.host', {
      slot: 'sidebar',
      component: SidebarHost,
      priority: 10,
    })
  },
}
