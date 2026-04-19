# Writing a Plugin

This guide walks through writing a plugin from scratch — a simple word count panel that shows the word count of the active file in a sidebar view and a status bar item.

---

## Step 1 — Plan your contributions

Before writing code, decide what your plugin contributes:

- **A sidebar view** — shows word count for the active file
- **A status bar item** — shows word count in the status bar at all times
- **A command** — `wordCount.toggle` to show/hide the sidebar view
- **A config section** — lets the user configure what to count

---

## Step 2 — Define the manifest

```typescript
// src/plugins/community/wordCount/index.ts

import type { Plugin } from '../../../types/plugin'
import { WordCountView } from './WordCountView'
import { WordCountStatusItem } from './WordCountStatusItem'

export const wordCountPlugin: Plugin = {
  manifest: {
    id: 'community.word-count',
    name: 'Word Count',
    version: '1.0.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['core.configuration-service'],

    contributes: {
      commands: [
        {
          id: 'wordCount.toggle',
          title: 'Toggle Word Count Panel',
          category: 'Word Count',
        }
      ],
      keybindings: [
        {
          command: 'wordCount.toggle',
          key: 'ctrl+shift+w',
          when: 'sidebarVisible',
        }
      ],
      views: [
        {
          id: 'wordCountPanel',
          slot: 'sidebar',
          title: 'Word Count',
          priority: 80,
        }
      ]
    }
  },

  activate(api) { /* step 3 */ },
  deactivate() { /* step 4 */ },
}
```

---

## Step 3 — Implement activate()

```typescript
activate(api: PluginAPI) {
  // Register the sidebar view component
  api.views.register('wordCountPanel', {
    slot: 'sidebar',
    component: WordCountView,
    priority: 80,
  })

  // Register the status bar item
  const statusItem = api.statusBar.createItem({
    id: 'wordCount.statusItem',
    slot: 'right',
    priority: 20,
    text: '0 words',
    tooltip: 'Word count',
    command: 'wordCount.toggle',
  })

  // Subscribe to file changes to update the count
  api.events.on('editor:contentChanged', ({ content }) => {
    const count = countWords(content)
    statusItem.text = `${count} words`
    api.context.set('wordCount.currentCount', count)
  })

  // Wire the toggle command
  api.commands.register('wordCount.toggle', () => {
    const visible = api.context.get('wordCount.panelVisible') ?? true
    api.context.set('wordCount.panelVisible', !visible)
  })

  // Register config section
  api.configuration.register({
    pluginId: 'community.word-count',
    title: 'Word Count',
    order: 100,
    schema: [
      {
        key: 'wordCount.includeCode',
        title: 'Include code blocks',
        type: 'boolean',
        default: false,
        description: 'Count words inside fenced code blocks',
      },
      {
        key: 'wordCount.includeHeadings',
        title: 'Include headings',
        type: 'boolean',
        default: true,
        description: 'Count words in heading lines',
      },
    ]
  })

  // Set initial context key
  api.context.set('wordCount.panelVisible', true)
},
```

---

## Step 4 — Implement deactivate()

Most cleanup is automatic (registry entries are swept on unload). You only need `deactivate()` for non-registry cleanup:

```typescript
deactivate() {
  // Nothing needed here — the registry sweep handles:
  //   - removing the sidebar view from the slot
  //   - removing the command from the palette
  //   - removing the keybinding
  //   - removing the status bar item
  //   - removing the config section
  //   - removing the event subscriptions (if tracked by the event bus)
}
```

---

## Step 5 — Write the view components

### WordCountView

```typescript
// src/plugins/community/wordCount/WordCountView.tsx

import { useContextKey } from '../../../host/ContextKeyService'
import { useConfigValue } from '../../../stores/configStore'

export function WordCountView() {
  const visible = useContextKey('wordCount.panelVisible')
  const count = useContextKey('wordCount.currentCount') as number ?? 0
  const includeCode = useConfigValue('wordCount.includeCode', false)

  if (!visible) return null

  return (
    <div className="word-count-panel">
      <div className="wc-header">Word Count</div>
      <div className="wc-stats">
        <div className="wc-stat">
          <span className="wc-label">Words</span>
          <span className="wc-value">{count.toLocaleString()}</span>
        </div>
      </div>
      <div className="wc-config-note">
        {includeCode ? 'Including code blocks' : 'Excluding code blocks'}
      </div>
    </div>
  )
}
```

### WordCountStatusItem

Status bar items are registered through the API — they don't need a full React component. The `text` property on the status item is reactive if you update it.

---

## Step 6 — Register the plugin

Add it to your plugin list in `main.tsx`:

```typescript
import { wordCountPlugin } from './plugins/community/wordCount'

const plugins = [
  // ... core plugins ...
  wordCountPlugin,  // ← add here
]

await host.loadAll(plugins)
```

---

## Common Patterns

### Reading config values in a component

```typescript
import { useConfigValue } from '../../../stores/configStore'

function MyComponent() {
  // Reactive — re-renders when the value changes in settings
  const fontSize = useConfigValue('myPlugin.fontSize', 14) as number
  return <div style={{ fontSize }}>...</div>
}
```

### Showing a modal

```typescript
// 1. Register a component into the overlay slot
api.views.register('myPlugin.myModal', {
  slot: 'overlay',
  component: MyModal,
  priority: 80,
})

// 2. Use a context key for visibility inside the component
function MyModal() {
  const visible = useContextKey('myPlugin.modalVisible')
  if (!visible) return null
  return (
    <div className="modal-backdrop" onClick={close}>
      <div className="modal" onClick={e => e.stopPropagation()}>
        ...
      </div>
    </div>
  )
}

// 3. Open it from a command
api.commands.register('myPlugin.openModal', () => {
  api.context.set('myPlugin.modalVisible', true)
})
```

### Emitting events for other plugins

```typescript
// Emit
api.events.emit('myPlugin:thingHappened', { detail: 'some value' })

// Another plugin subscribes:
api.events.on('myPlugin:thingHappened', ({ detail }) => {
  console.log('got:', detail)
})
```

### Reacting to editor events

```typescript
api.events.on('editor:activeFileChanged', ({ path, content }) => {
  // update your view
})

api.events.on('editor:contentChanged', ({ path, content, delta }) => {
  // content updated
})
```

---

## Plugin Checklist

Before shipping a plugin:

- [ ] `id` follows `org.plugin-name` convention
- [ ] `dependsOn` lists all required services
- [ ] All commands in manifest have handlers registered in `activate()`
- [ ] All views registered in `activate()` match IDs in manifest `contributes.views`
- [ ] Context keys are namespaced: `myPlugin.keyName`
- [ ] Config keys are namespaced: `myPlugin.settingName`
- [ ] No direct `document.body` manipulation
- [ ] No direct imports from other plugins (use events instead)
- [ ] No hardcoded colors (use CSS custom properties)
- [ ] `deactivate()` cleans up anything outside the registry
