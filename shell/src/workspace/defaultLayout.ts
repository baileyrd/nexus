// Default workspace layout — used when <vault>/.forge/workspace.json is
// absent, corrupt, or fails schema validation.
//
// Plan reference: /home/baileyrd/projects/nexus/docs/leaf-migration-plan.md
// §Phase 6. Matches the visual layout the pre-migration shell produced so
// first-run users see no regression:
//   - left dock: file-explorer, then search (tabs)
//   - right dock: outline, then backlink (tabs)
//   - main: a single tabs group with one `empty` leaf; editor mounts on
//           file-open
//
// Stable ids are generated with crypto.randomUUID(); the leaf inside the
// main tabs becomes the initial `active` leaf.

import type {
  SerializedLeaf,
  SerializedSplit,
  SerializedTabs,
  WorkspaceJSON,
} from './types.ts'

const newId = (): string => crypto.randomUUID()

function makeLeaf(type: string): SerializedLeaf {
  return {
    kind: 'leaf',
    id: newId(),
    viewState: { type },
  }
}

function makeTabs(leafTypes: string[]): SerializedTabs {
  return {
    kind: 'tabs',
    id: newId(),
    leaves: leafTypes.map(makeLeaf),
    activeIndex: 0,
  }
}

/**
 * Build the default WorkspaceJSON. Called once at boot when no saved
 * layout exists.
 */
export function buildDefaultLayout(): WorkspaceJSON {
  const mainTabs = makeTabs(['empty'])
  const mainSplit: SerializedSplit = {
    kind: 'split',
    id: newId(),
    direction: 'horizontal',
    children: [mainTabs],
  }

  const leftTabs = makeTabs(['file-explorer', 'search', 'bookmarks'])
  const leftDock: SerializedSplit = {
    kind: 'split',
    id: newId(),
    direction: 'vertical',
    children: [leftTabs],
    side: 'left',
    collapsed: false,
    size: 260,
  }

  // Order mirrors Obsidian's default right-dock tab order:
  //   backlinks → outgoing-links → tags → all-properties → outline
  //   → file-properties → bookmarks
  const rightTabs = makeTabs([
    'backlink',
    'outgoing-links',
    'tags',
    'all-properties',
    'outline',
    'file-properties',
    'bookmarks',
  ])
  const rightDock: SerializedSplit = {
    kind: 'split',
    id: newId(),
    direction: 'vertical',
    children: [rightTabs],
    side: 'right',
    collapsed: false,
    size: 280,
  }

  // Bottom drawer: terminal + future build-log / problems panes live
  // here. Starts collapsed — an expanded-by-default drawer is intrusive
  // for users who never open a terminal (matches VS Code behavior).
  const bottomTabs = makeTabs(['empty'])
  const bottomDock: SerializedSplit = {
    kind: 'split',
    id: newId(),
    direction: 'horizontal',
    children: [bottomTabs],
    side: 'bottom',
    collapsed: true,
    size: 240,
  }

  const activeLeafId = mainTabs.leaves[0]!.id

  return {
    main: mainSplit,
    left: leftDock,
    right: rightDock,
    bottom: bottomDock,
    active: activeLeafId,
    lastOpenFiles: [],
  }
}
