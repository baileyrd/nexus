# Event Bus

The event bus is the communication layer between plugins. Plugins never import each other directly — they emit and subscribe to typed events through the bus.

---

## Why an Event Bus?

Direct imports between plugins create tight coupling and circular dependency problems. If `core.file-explorer` imports from `core.editor-area` to open a file, then the editor plugin can never be replaced without modifying the explorer. With an event bus:

- Explorer emits `fileExplorer:fileSelected` with the file path
- Editor subscribes to `fileExplorer:fileSelected` and opens the file
- Neither plugin knows the other exists

Plugins become independently replaceable.

---

## Implementation

```typescript
// src/host/EventBus.ts

type Handler<T = unknown> = (payload: T) => void

class EventBus {
  private handlers = new Map<string, Set<Handler>>()

  on<T = unknown>(event: string, handler: Handler<T>): () => void {
    if (!this.handlers.has(event)) {
      this.handlers.set(event, new Set())
    }
    this.handlers.get(event)!.add(handler as Handler)

    // Return unsubscribe function
    return () => {
      this.handlers.get(event)?.delete(handler as Handler)
    }
  }

  emit<T = unknown>(event: string, payload: T): void {
    const handlers = this.handlers.get(event)
    if (!handlers) return

    for (const handler of handlers) {
      try {
        handler(payload)
      } catch (err) {
        console.error(`Event handler for '${event}' threw:`, err)
      }
    }
  }

  // Wildcard subscription — receives all events
  onAll(handler: (event: string, payload: unknown) => void): () => void {
    return this.on('*', ({ event, payload }) => handler(event as string, payload))
  }
}

export const eventBus = new EventBus()
```

---

## Event Naming Conventions

Events are namespaced by source: `source:eventName`

```
editor:activeFileChanged     ← emitted by core.editor-area
editor:contentChanged
editor:fileSaved
fs:fileCreated               ← emitted by core.filesystem-service
fs:fileDeleted
fs:fileChanged
plugin:activated             ← emitted by extension host
plugin:deactivated
shell:themeChanged           ← emitted by core.theme-service
myPlugin:customEvent         ← community plugin events
```

### Conventions

- `source` is the plugin's abbreviated name (not the full ID)
- `eventName` is camelCase
- Events describe what happened, not what should happen (`file:opened` not `open:file`)
- Past tense for completed actions (`file:saved`), present for ongoing (`editor:typing`)

---

## Subscription Lifecycle

Subscriptions should be cleaned up when a plugin unloads. The cleanest way is to collect unsubscribe functions and call them in `deactivate()`:

```typescript
// Module-level cleanup array (since plugin objects aren't classes)
const cleanups: Array<() => void> = []

activate(api: PluginAPI) {
  cleanups.push(
    api.events.on('editor:activeFileChanged', ({ path }) => {
      // handle
    })
  )
  cleanups.push(
    api.events.on('fs:fileChanged', ({ path }) => {
      // handle
    })
  )
}

deactivate() {
  cleanups.forEach(fn => fn())
  cleanups.length = 0
}
```

Alternatively, if the event bus is integrated with the plugin registry's ownership tracking, subscriptions can be swept automatically on unload like registry contributions are.

---

## Synchronous vs Asynchronous

The event bus above is synchronous — `emit()` runs all handlers before returning. This is appropriate for most shell events where handlers need to react immediately.

For events where handlers should not block the emitter (e.g., a file watcher emitting many events rapidly), emit asynchronously:

```typescript
emitAsync<T>(event: string, payload: T): void {
  setTimeout(() => this.emit(event, payload), 0)
}
```

---

## Using Events for Cross-Plugin Communication

### Pattern 1 — Notification

Plugin A tells anyone who cares that something happened. No response expected.

```typescript
// Plugin A
api.events.emit('myPlugin:analysisComplete', { filePath, results })

// Plugin B (optional subscriber)
api.events.on('myPlugin:analysisComplete', ({ filePath, results }) => {
  updateUI(filePath, results)
})
```

### Pattern 2 — Request/Response via Context Keys

For cases where plugin A needs data from plugin B, use context keys as the response channel:

```typescript
// Plugin A requests
api.events.emit('editor:requestContent', { requestId: '123' })

// Plugin B responds
api.events.on('editor:requestContent', ({ requestId }) => {
  const content = getEditorContent()
  api.context.set(`editor:contentResponse:${requestId}`, content)
})

// Plugin A reads response
const content = api.context.get('editor:contentResponse:123')
```

This pattern avoids circular event dependencies. For real two-way communication, the `api.internal` service system is cleaner.

### Pattern 3 — Broadcast state changes

```typescript
// When active file changes, broadcast to all interested plugins
api.events.emit('editor:activeFileChanged', {
  path: newPath,
  content: newContent,
  language: detectedLanguage,
})

// Word count plugin
api.events.on('editor:activeFileChanged', ({ content }) => {
  updateWordCount(content)
})

// Outline plugin
api.events.on('editor:activeFileChanged', ({ content, language }) => {
  rebuildOutline(content, language)
})

// Breadcrumbs plugin
api.events.on('editor:activeFileChanged', ({ path }) => {
  updateBreadcrumbs(path)
})
```

All three plugins receive the same event and independently update. None knows about the others.
