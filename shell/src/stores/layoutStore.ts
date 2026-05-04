// src/stores/layoutStore.ts
// Residual shell settings that do not (yet) live in the workspace tree.
//
// History: this store used to own sidebar / rightPanel / panelArea
// visibility, widths, and active view ids. All of those migrated to
// `workspace` (see shell/src/workspace/workspaceStore.ts). The fields
// that remain here are body-class toggles the workspace model has no
// representation for:
//
//   - activityBar.visible — drives the `show-ribbon` body class; the
//     activity bar is chrome, not a workspace node.
//   - showViewHeader     — drives the `show-view-header` body class;
//     a settings-level toggle for per-leaf view headers (Obsidian
//     parity), not a spatial control.
//
// Follow-up task #11 (leaf-migration): a future panel-area concept will
// restore a bottom-dock to the workspace tree. When that lands the
// `activityBar`/`showViewHeader` flags will either move onto a proper
// settings store or stay here — whichever is cleaner at that point.

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface LayoutStore {
  activityBar: { visible: boolean }
  // `show-view-header` body class (Obsidian parity). Controls per-pane
  // view headers; toggled via settings, not a spatial control.
  showViewHeader: boolean

  toggleViewHeader: () => void
}

export const useLayoutStore = create<LayoutStore>()(
  persist(
    (set) => ({
      activityBar: { visible: true },
      showViewHeader: true,

      toggleViewHeader: () => set(s => ({ showViewHeader: !s.showViewHeader })),
    }),
    {
      name: 'shell-layout',
      // v3: leaf-migration cleanup — sidebar / rightPanel / panelArea
      // fields retired (now live in the workspace tree). Bumping the
      // version drops any persisted state from v2.
      version: 3,
      migrate: (_persisted, _version) => ({
        activityBar: { visible: true },
        showViewHeader: true,
      } as LayoutStore),
      merge: (persisted, current) => ({ ...current, ...(persisted as Partial<LayoutStore>) }),
    }
  )
)
