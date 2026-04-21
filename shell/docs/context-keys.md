# Context Keys

Context keys are the application's shared ambient state. They drive two things:

1. **Command enablement** — `when: 'editorFocus && !readOnly'` on a command means it only appears enabled when those keys match
2. **Conditional rendering** — components read context keys to decide whether to show themselves (`commandPaletteVisible`, `settingsPanelVisible`)

---

## Implementation

```typescript
// src/host/ContextKeyService.ts
import { create } from 'zustand'

interface ContextKeyStore {
  keys: Record<string, unknown>
  set: (key: string, value: unknown) => void
  get: (key: string) => unknown
  evaluate: (expression: string) => boolean
}

export const useContextKeyStore = create<ContextKeyStore>((set, get) => ({
  keys: {},

  set: (key, value) =>
    set(s => ({ keys: { ...s.keys, [key]: value } })),

  get: (key) => get().keys[key],

  // Evaluate a when-clause expression against current keys
  evaluate: (expression) => {
    if (!expression) return true
    return evaluateExpression(expression, get().keys)
  }
}))

// Convenience hook for reading a single key
export function useContextKey(key: string): unknown {
  return useContextKeyStore(s => s.keys[key])
}
```

---

## When-Clause Expressions

When-clause expressions are simple boolean expressions over context key names.

### Operators

| Operator | Example | Meaning |
|---|---|---|
| (none) | `editorFocus` | key is truthy |
| `!` | `!readOnly` | key is falsy |
| `&&` | `editorFocus && !readOnly` | both conditions |
| `\|\|` | `sidebarFocus \|\| explorerFocus` | either condition |
| `==` | `fileExtension == 'md'` | key equals value |
| `!=` | `fileExtension != 'json'` | key does not equal value |
| `=~` | `resourcePath =~ /\.test\./` | key matches regex |

### Expression evaluator

```typescript
function evaluateExpression(
  expression: string,
  keys: Record<string, unknown>
): boolean {
  // Simple recursive descent parser
  // Handles: &&, ||, !, ==, !=, =~, parentheses
  // Returns boolean

  // ... implementation ...
  // In practice, a small library like 'expr-eval' works well here,
  // or a hand-rolled recursive descent parser for the subset of
  // operators above.
}
```

---

## Built-in Context Keys

The shell substrate and core plugins set these keys. Plugin authors can read them and write `when` expressions against them.

### Shell-level keys

| Key | Type | Set by | Meaning |
|---|---|---|---|
| `shellReady` | `boolean` | Shell on startup | Shell has fully initialized |
| `os` | `'windows' \| 'macos' \| 'linux'` | Shell on startup | Current operating system |

### UI state keys (set by core UI plugins)

| Key | Type | Set by | Meaning |
|---|---|---|---|
| `sidebarVisible` | `boolean` | `core.sidebar` | Sidebar panel is open |
| `sidebarFocus` | `boolean` | `core.sidebar` | Focus is in the sidebar |
| `panelAreaVisible` | `boolean` | `core.panel-area` | Bottom panel is open |
| `editorFocus` | `boolean` | `core.editor-area` | Focus is in an editor |
| `editorReadOnly` | `boolean` | `core.editor-area` | Active editor is read-only |
| `activeFileExtension` | `string` | `core.editor-area` | Extension of active file |
| `activeLanguage` | `string` | `core.editor-area` | Language of active file |
| `terminalFocus` | `boolean` | `core.terminal` | Focus is in the terminal |

### Modal/overlay keys

| Key | Type | Set by | Meaning |
|---|---|---|---|
| `commandPaletteVisible` | `boolean` | `core.command-palette` | Palette is open |
| `settingsPanelVisible` | `boolean` | `core.settings` | Settings panel is open |

---

## Setting Context Keys from a Plugin

```typescript
activate(api: PluginAPI) {
  // Register a new context key with its initial value
  api.context.set('myPlugin.isConnected', false)

  // Update it when state changes
  connectToServer().then(() => {
    api.context.set('myPlugin.isConnected', true)
  })
}
```

### Naming convention

Use `pluginId.keyName` format to avoid collisions:

```
myPlugin.isConnected    ✓  namespaced, clear ownership
isConnected             ✗  could collide with another plugin
```

Core shell keys use flat names (`editorFocus`, `sidebarVisible`) as they're part of the public contract.

---

## Using Context Keys in Commands

```typescript
// In manifest:
contributes: {
  commands: [{
    id: 'myPlugin.formatDocument',
    title: 'Format Document',
  }],
  keybindings: [{
    command: 'myPlugin.formatDocument',
    key: 'shift+alt+f',
    when: 'editorFocus && activeLanguage == typescript',
  }]
}
```

The keybinding only fires when an editor has focus AND the active file is TypeScript.

---

## Using Context Keys for Visibility

The canonical pattern for modal overlay components:

```typescript
function MyModalView() {
  const visible = useContextKey('myPlugin.modalVisible')

  // Always mounted, renders null when not visible
  if (!visible) return null

  return (
    <div className="my-modal-backdrop" onClick={close}>
      <div className="my-modal" onClick={e => e.stopPropagation()}>
        {/* modal content */}
      </div>
    </div>
  )
}

// To open:
api.context.set('myPlugin.modalVisible', true)

// To close (from inside the component):
useContextKeyStore.getState().set('myPlugin.modalVisible', false)
```

---

## Context Keys and Focus Management

The shell maintains a focus stack. When a modal opens and captures focus, its context key goes `true`. When it closes and focus is restored, the key goes `false`. This is how `when: 'editorFocus'` can be `false` while a modal is open even though the editor is still visible — it doesn't have focus.

```typescript
// In a modal component's focus trap:
useEffect(() => {
  if (visible) {
    // Save current focus before taking it
    previousFocus.current = document.activeElement as HTMLElement
    containerRef.current?.focus()
    api.context.set('commandPaletteVisible', true)
    api.context.set('editorFocus', false)  // editor lost focus
  } else {
    // Restore focus on close
    previousFocus.current?.focus()
    api.context.set('commandPaletteVisible', false)
    // editorFocus will be set true again by the editor's own focus handler
  }
}, [visible])
```
