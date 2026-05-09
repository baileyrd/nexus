# Registry System

The plugin registry is a collection of sub-registries, one per contribution type. It is the central catalog of everything plugins have contributed to the shell.

---

## PluginRegistry — The Root

```typescript
// src/host/PluginRegistry.ts

export class PluginRegistry {
  readonly commands    = new CommandRegistry()
  readonly views       = new ViewRegistry()
  readonly menus       = new MenuRegistry()
  readonly keybindings = new KeybindingRegistry()
  readonly statusBar   = new StatusBarRegistry()
  readonly slots       = new SlotRegistry()
  readonly config      = new ConfigurationRegistry()

  // Reverse index: pluginId → Set of contribution keys
  // Used to sweep all contributions when a plugin unloads
  private ownership = new Map<string, Set<string>>()

  track(pluginId: string, contributionKey: string) {
    if (!this.ownership.has(pluginId)) {
      this.ownership.set(pluginId, new Set())
    }
    this.ownership.get(pluginId)!.add(contributionKey)
  }

  unregisterAll(pluginId: string) {
    const keys = this.ownership.get(pluginId)
    if (!keys) return

    for (const key of keys) {
      // Each key is prefixed: 'command:id', 'view:id', 'slot:slotId:entryId', etc.
      const [type, ...rest] = key.split(':')
      const id = rest.join(':')
      switch (type) {
        case 'command':    this.commands.unregister(id); break
        case 'view':       this.views.unregister(id); break
        case 'slot':       this.slots.unregister(id); break
        case 'statusBar':  this.statusBar.unregister(id); break
        case 'config':     this.config.unregister(id); break
        case 'keybinding': this.keybindings.unregister(id); break
      }
    }

    this.ownership.delete(pluginId)
  }
}
```

---

## CommandRegistry

Stores all registered commands. A command has an ID, a label, an optional handler, and an optional `when` expression.

The manifest registers the label (so the command palette can show it). `activate()` registers the handler. A command can exist in the registry without a handler — it just won't execute until one is wired.

```typescript
// src/registry/CommandRegistry.ts

interface CommandEntry {
  id: string
  title: string
  category?: string
  handler?: (...args: unknown[]) => unknown
  when?: string           // context expression for enablement
  pluginId: string
}

export class CommandRegistry {
  private commands = new Map<string, CommandEntry>()

  registerFromManifest(pluginId: string, contribution: CommandContribution) {
    this.commands.set(contribution.id, {
      ...contribution,
      pluginId,
      handler: undefined,  // handler comes later in activate()
    })
  }

  register(pluginId: string, id: string, handler: (...args: unknown[]) => unknown) {
    const existing = this.commands.get(id)
    if (existing) {
      existing.handler = handler  // wire handler to manifest entry
    } else {
      this.commands.set(id, { id, title: id, pluginId, handler })
    }
  }

  unregister(id: string) {
    this.commands.delete(id)
  }

  async execute(id: string, ...args: unknown[]) {
    const cmd = this.commands.get(id)
    if (!cmd?.handler) {
      console.warn(`Command '${id}' has no handler`)
      return
    }
    return cmd.handler(...args)
  }

  all(): CommandEntry[] {
    return [...this.commands.values()]
  }

  get(id: string): CommandEntry | undefined {
    return this.commands.get(id)
  }
}
```

---

## SlotRegistry

The most important registry for rendering. It maps slot IDs to sorted lists of plugin-contributed components.

```typescript
// src/registry/SlotRegistry.ts
import { create } from 'zustand'

export type SlotId =
  | 'overlay'
  | 'titleBar'
  | 'activityBar'
  | 'sidebar'
  | 'editorArea'
  | 'panelArea'
  | 'statusBarLeft'
  | 'statusBarRight'

export interface SlotEntry {
  id: string                          // unique entry ID
  pluginId: string                    // owning plugin
  component: React.ComponentType<any> // what to render
  priority: number                    // lower = rendered first/higher
}

interface SlotStore {
  slots: Record<SlotId, SlotEntry[]>
  register: (slotId: SlotId, entry: SlotEntry) => void
  unregister: (entryId: string) => void
}

export const useSlotStore = create<SlotStore>((set) => ({
  slots: {
    overlay: [],
    titleBar: [],
    activityBar: [],
    sidebar: [],
    editorArea: [],
    panelArea: [],
    statusBarLeft: [],
    statusBarRight: [],
  },

  register: (slotId, entry) =>
    set(s => ({
      slots: {
        ...s.slots,
        [slotId]: [...s.slots[slotId], entry]
          .sort((a, b) => a.priority - b.priority)
      }
    })),

  unregister: (entryId) =>
    set(s => ({
      slots: Object.fromEntries(
        Object.entries(s.slots).map(([k, entries]) => [
          k,
          (entries as SlotEntry[]).filter(e => e.id !== entryId)
        ])
      ) as Record<SlotId, SlotEntry[]>
    }))
}))
```

---

## ViewRegistry

Tracks view metadata — display names, target slots, and which plugin owns them. Separate from the SlotRegistry because a view can be declared in the manifest (metadata only) before the component is registered in `activate()`.

```typescript
// src/registry/ViewRegistry.ts

interface ViewEntry {
  id: string
  pluginId: string
  slot: SlotId
  title: string
  priority: number
  component?: React.ComponentType<any>  // set during activate()
}

export class ViewRegistry {
  private views = new Map<string, ViewEntry>()

  registerFromManifest(pluginId: string, contribution: ViewContribution) {
    this.views.set(contribution.id, {
      ...contribution,
      pluginId,
      component: undefined,
    })
  }

  // Called from activate() to attach the component
  registerComponent(viewId: string, component: React.ComponentType<any>) {
    const view = this.views.get(viewId)
    if (view) {
      view.component = component
    }
  }

  unregister(id: string) {
    this.views.delete(id)
  }

  get(id: string): ViewEntry | undefined {
    return this.views.get(id)
  }

  all(): ViewEntry[] {
    return [...this.views.values()]
  }
}
```

---

## KeybindingRegistry

Manages chord → command mappings with optional `when` context conditions.

```typescript
// src/registry/KeybindingRegistry.ts

interface KeybindingEntry {
  id: string         // unique ID for this binding
  pluginId: string
  chord: string      // normalized: 'ctrl+shift+p'
  commandId: string
  when?: string      // context expression
}

export class KeybindingRegistry {
  private bindings: KeybindingEntry[] = []

  registerFromManifest(pluginId: string, contribution: KeybindingContribution) {
    const isMac = navigator.platform.toLowerCase().includes('mac')
    const chord = (isMac && contribution.mac) ? contribution.mac : contribution.key

    this.bindings.push({
      id: `${pluginId}:${contribution.command}`,
      pluginId,
      chord: normalizeChord(chord),
      commandId: contribution.command,
      when: contribution.when,
    })
  }

  unregister(id: string) {
    this.bindings = this.bindings.filter(b => b.id !== id)
  }

  // Find the command for a keydown event
  match(event: KeyboardEvent, contextKeys: Record<string, unknown>): string | null {
    const chord = eventToChord(event)

    for (const binding of this.bindings) {
      if (binding.chord !== chord) continue
      if (binding.when && !evaluateWhen(binding.when, contextKeys)) continue
      return binding.commandId
    }

    return null
  }
}
```

---

## ConfigurationRegistry

Stores plugin-declared configuration schemas. The settings panel UI plugin reads from this to auto-generate settings screens.

```typescript
// src/registry/ConfigurationRegistry.ts

export interface ConfigSchema {
  key: string
  title: string
  description: string
  type: 'boolean' | 'string' | 'number' | 'select' | 'keybinding'
  default: unknown
  options?: string[]   // for 'select' type
  when?: string        // only show when context expression is true
}

export interface ConfigSection {
  pluginId: string
  title: string
  order: number
  schema: ConfigSchema[]
}

export class ConfigurationRegistry {
  private sections = new Map<string, ConfigSection>()

  register(section: ConfigSection) {
    this.sections.set(section.pluginId, section)
  }

  unregister(pluginId: string) {
    this.sections.delete(pluginId)
  }

  all(): ConfigSection[] {
    return [...this.sections.values()]
      .sort((a, b) => a.order - b.order)
  }

  get(pluginId: string): ConfigSection | undefined {
    return this.sections.get(pluginId)
  }
}
```

---

## StatusBarRegistry

```typescript
// src/registry/StatusBarRegistry.ts

export interface StatusBarItem {
  id: string
  pluginId: string
  slot: 'left' | 'right'
  priority: number         // lower = closer to edge
  text: string | (() => string)
  tooltip?: string
  command?: string         // command to execute on click
  color?: string
}

export class StatusBarRegistry {
  private items = new Map<string, StatusBarItem>()

  create(pluginId: string, config: Omit<StatusBarItem, 'pluginId'>): StatusBarItem {
    const item = { ...config, pluginId }
    this.items.set(config.id, item)
    return item
  }

  update(id: string, updates: Partial<StatusBarItem>) {
    const item = this.items.get(id)
    if (item) Object.assign(item, updates)
  }

  unregister(id: string) {
    this.items.delete(id)
  }

  getSlot(slot: 'left' | 'right'): StatusBarItem[] {
    return [...this.items.values()]
      .filter(i => i.slot === slot)
      .sort((a, b) => a.priority - b.priority)
  }
}
```

---

## Ownership Tracking

Every registration call goes through the `PluginRegistry.track()` method:

```typescript
// In buildPluginAPI(), every API method that registers something tracks it:

commands: {
  register: (id, handler) => {
    registry.commands.register(pluginId, id, handler)
    registry.track(pluginId, `command:${id}`)
  }
},

views: {
  register: (viewId, config) => {
    registry.slots.register(config.slot, {
      id: viewId,
      pluginId,
      component: config.component,
      priority: config.priority ?? 50,
    })
    registry.track(pluginId, `slot:${viewId}`)
  }
}
```

When `registry.unregisterAll(pluginId)` is called (on plugin unload), it iterates the tracked keys and removes every contribution. This is automatic — plugin authors do not need to manually clean up registry entries.
