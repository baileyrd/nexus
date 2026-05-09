# Plugin API Reference

The plugin API object is passed to every plugin's `activate()` function. Core plugins receive the full surface including `api.internal`. Community plugins receive everything except `api.internal`.

---

## api.commands

```typescript
interface CommandsAPI {
  // Register a handler for a command ID
  // The command must already exist in the manifest OR be registered fresh here
  register(id: string, handler: (...args: unknown[]) => unknown): void

  // Execute a command programmatically
  execute(id: string, ...args: unknown[]): Promise<unknown>

  // Get all registered commands (for building custom palettes)
  all(): CommandEntry[]
}
```

### Example

```typescript
api.commands.register('myPlugin.doThing', async (arg?: string) => {
  const result = await doSomething(arg)
  return result
})

// Execute from within a plugin:
await api.commands.execute('myPlugin.doThing', 'some-arg')
```

---

## api.views

```typescript
interface ViewsAPI {
  register(viewId: string, config: {
    slot: SlotId
    component: React.ComponentType<any>
    priority?: number      // default 50
  }): void
}
```

### Example

```typescript
api.views.register('myPlugin.panel', {
  slot: 'sidebar',
  component: MyPanelComponent,
  priority: 40,
})
```

---

## api.context

```typescript
interface ContextAPI {
  // Set a context key value
  set(key: string, value: unknown): void

  // Get a context key value
  get(key: string): unknown

  // Evaluate a when-clause expression
  evaluate(expression: string): boolean
}
```

### Example

```typescript
api.context.set('myPlugin.isConnected', true)
const connected = api.context.get('myPlugin.isConnected') // true
const canSave = api.context.evaluate('editorFocus && !editorReadOnly') // boolean
```

---

## api.events

```typescript
interface EventsAPI {
  // Subscribe to an event
  on<T = unknown>(event: string, handler: (payload: T) => void): () => void

  // Emit an event
  emit<T = unknown>(event: string, payload: T): void
}
```

The return value of `on()` is an unsubscribe function. Call it in `deactivate()` if the event bus doesn't track subscriptions automatically.

### Example

```typescript
const unsub = api.events.on('file:opened', ({ path }) => {
  console.log('opened:', path)
})

api.events.emit('myPlugin:ready', { version: '1.0.0' })
```

### Built-in events

| Event | Payload | Emitted by |
|---|---|---|
| `editor:activeFileChanged` | `{ path: string, content: string }` | `core.editor-area` |
| `editor:contentChanged` | `{ path: string, content: string }` | `core.editor-area` |
| `editor:fileSaved` | `{ path: string }` | `core.editor-area` |
| `fs:fileCreated` | `{ path: string }` | `core.filesystem-service` |
| `fs:fileDeleted` | `{ path: string }` | `core.filesystem-service` |
| `fs:fileChanged` | `{ path: string }` | `core.filesystem-service` |
| `plugin:activated` | `{ pluginId: string }` | Extension host |
| `plugin:deactivated` | `{ pluginId: string }` | Extension host |

---

## api.configuration

Available after `core.configuration-service` has loaded.

```typescript
interface ConfigurationAPI {
  // Register a config section — populates the settings panel
  register(section: ConfigSection): void

  // Read a config value
  getValue<T>(key: string, defaultValue: T): T

  // Write a config value (also available from the settings panel UI)
  setValue(key: string, value: unknown): void

  // Subscribe to changes on a specific key
  onChange(key: string, handler: (newValue: unknown) => void): () => void
}
```

### Example

```typescript
api.configuration.register({
  pluginId: 'my-org.my-plugin',
  title: 'My Plugin',
  order: 50,
  schema: [
    {
      key: 'myPlugin.theme',
      title: 'Color theme',
      type: 'select',
      options: ['light', 'dark', 'system'],
      default: 'system',
      description: 'Choose the color theme for My Plugin',
    }
  ]
})

const theme = api.configuration.getValue('myPlugin.theme', 'system')

api.configuration.onChange('myPlugin.theme', (newTheme) => {
  applyTheme(newTheme as string)
})
```

---

## api.statusBar

```typescript
interface StatusBarAPI {
  createItem(config: {
    id: string
    slot: 'left' | 'right'
    priority: number
    text: string
    tooltip?: string
    command?: string
  }): StatusBarItemHandle
}

interface StatusBarItemHandle {
  text: string           // settable — updates immediately
  tooltip: string        // settable
  dispose(): void        // remove from status bar
}
```

### Example

```typescript
const item = api.statusBar.createItem({
  id: 'myPlugin.statusItem',
  slot: 'right',
  priority: 30,
  text: 'Ready',
  command: 'myPlugin.showDetails',
})

// Update later:
item.text = 'Processing...'
```

---

## api.notifications

Available after `core.notification-service` has loaded.

```typescript
interface NotificationsAPI {
  show(notification: {
    message: string
    type?: 'info' | 'warning' | 'error' | 'success'  // default: 'info'
    duration?: number       // ms before auto-dismiss. 0 = no auto-dismiss
    actions?: Array<{
      label: string
      command: string
    }>
  }): void
}
```

### Example

```typescript
api.notifications.show({
  message: 'File saved successfully',
  type: 'success',
  duration: 3000,
})

api.notifications.show({
  message: 'Build failed — 3 errors',
  type: 'error',
  duration: 0,  // persist until dismissed
  actions: [
    { label: 'View errors', command: 'problems.focusPanel' }
  ]
})
```

---

## api.fs

Available after `core.filesystem-service` has loaded.

```typescript
interface FilesystemAPI {
  read(path: string): Promise<string>
  write(path: string, content: string): Promise<void>
  list(path: string): Promise<FileEntry[]>
  watch(path: string, handler: (event: FsEvent) => void): Promise<() => void>
  exists(path: string): Promise<boolean>
  mkdir(path: string): Promise<void>
  delete(path: string): Promise<void>
  rename(from: string, to: string): Promise<void>
}
```

---

## api.storage

Per-plugin persistent key-value storage. Scoped to the plugin ID — no plugin can read another plugin's storage.

```typescript
interface StorageAPI {
  get(key: string): string | null
  set(key: string, value: string): void
  delete(key: string): void
  clear(): void
}
```

### Example

```typescript
api.storage.set('lastOpenedFile', '/path/to/file.md')
const last = api.storage.get('lastOpenedFile')
```

---

## api.internal (core plugins only)

```typescript
interface InternalAPI {
  // Register a named service available to other core plugins via the API
  registerInternalService(name: string, service: unknown): void

  // Get a service registered by another core plugin
  getInternalService<T>(name: string): T

  // Define a new slot ID (shell starts with a fixed set — core plugins can add more)
  defineSlot(slotId: string): void

  // Direct access to the plugin registry
  registry: PluginRegistry
}
```

`api.internal` is `undefined` for community plugins. Any access attempt in community plugin code throws at the API boundary.
