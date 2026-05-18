> **Archived 2026-04-26** — Refactor plan; the keybinding override persistence has been moved out of the settings plugin. Current architecture lives in `docs/shell/extension-host.md` and `docs/shell/registry-system.md`.

# Plan: Move Keybinding Override Persistence to Registry Bootstrap

## Problem

`core/settings` currently owns two concerns it shouldn't:

1. **`keybindingOverrideStorage`** — a localStorage read/write adapter defined in `SettingsPanelView.tsx` (lines 237–262) and exported so `settings/index.ts` can import it.
2. **Hydration on activate** — `settings/index.ts` (lines 81–93) calls `reg.keybindings.loadOverrides(keybindingOverrideStorage)` via an unsafe `api.internal.registry` type cast.

These are lifecycle/persistence concerns that belong in the shell bootstrap, not in a UI plugin.

Additionally, every call to `setOverride`/`clearOverride` in the settings panel passes `keybindingOverrideStorage` as an argument — the registry has no internal storage reference of its own.

## Target State

- `KeybindingRegistry` owns a `storage` instance internally, bound at construction time via a `bindStorage()` call in the bootstrap.
- `main.tsx` creates the storage object, binds it, and calls `loadOverrides()` before plugins load.
- `SettingsPanelView` calls `setOverride(commandId, chord)` / `clearOverride(commandId)` — no storage argument needed.
- `settings/index.ts` has zero keybinding-persistence concerns.

---

## Steps

### Step 1 — Refactor `KeybindingRegistry` to own storage internally

**File:** `shell/src/registry/KeybindingRegistry.ts`

Add a private `storage: OverrideStorage | null = null` field.

Add a `bindStorage(storage: OverrideStorage): void` method (warn but don't throw on double-call).

Remove the `storage` parameter from `loadOverrides`, `setOverride`, `clearOverride`, and the private `persist` method. All four now read from `this.storage`; if null, log a warning and return early.

```ts
// Before:
async loadOverrides(storage: OverrideStorage): Promise<void>
async setOverride(storage: OverrideStorage, commandId: string, chord: string): Promise<void>
async clearOverride(storage: OverrideStorage, commandId: string): Promise<void>
private async persist(storage: OverrideStorage): Promise<void>

// After:
bindStorage(storage: OverrideStorage): void
async loadOverrides(): Promise<void>
async setOverride(commandId: string, chord: string): Promise<void>
async clearOverride(commandId: string): Promise<void>
private async persist(): Promise<void>
```

Update the comment on line ~36 (currently says "settings plugin uses `api.storage`") to reflect bootstrap ownership.

---

### Step 2 — Create `keybindingOverrideStorage.ts` in the registry layer

**New file:** `shell/src/registry/keybindingOverrideStorage.ts`

Move the localStorage adapter out of `SettingsPanelView.tsx`. Keep the storage key identical to preserve existing persisted user data.

```ts
import type { OverrideStorage } from './KeybindingRegistry'

const OVERRIDES_STORAGE_KEY = 'plugin:core.settings:keybinding-overrides'

export const keybindingOverrideStorage: OverrideStorage = {
  async read() { /* same implementation */ },
  async write(overrides) { /* same implementation */ },
}
```

> **Important:** The key `plugin:core.settings:keybinding-overrides` must not be renamed — changing it would silently discard every existing user's saved shortcuts.

---

### Step 3 — Wire storage in `main.tsx` before plugins load

**File:** `shell/src/main.tsx`

Inside `boot()`, immediately after `const reg = new PluginRegistry()`:

```ts
import { keybindingOverrideStorage } from './registry/keybindingOverrideStorage'

const reg = new PluginRegistry()
reg.keybindings.bindStorage(keybindingOverrideStorage)
void reg.keybindings.loadOverrides()   // must happen before host.loadAll(plugins)
```

Placing `loadOverrides()` before `host.loadAll()` ensures overrides are in the map before manifest contributions are registered, which hits the efficient pre-registration path (already tested by "loadOverrides applied before manifest registration also takes effect").

---

### Step 4 — Clean up `settings/index.ts`

**File:** `shell/src/plugins/core/settings/index.ts`

- Remove the `keybindingOverrideStorage` named import from line 8.
- Delete lines 81–93 (the `reg` type cast block and `loadOverrides` call) entirely.

No other changes.

---

### Step 5 — Clean up `SettingsPanelView.tsx`

**File:** `shell/src/plugins/core/settings/SettingsPanelView.tsx`

Remove:
- Lines 237–262: `OVERRIDES_STORAGE_KEY` constant and `export const keybindingOverrideStorage` block.

Update the two call sites in `KeybindingsTab`:

```ts
// Before:
await reg.keybindings.setOverride(keybindingOverrideStorage, commandId, chord)
await reg.keybindings.clearOverride(keybindingOverrideStorage, commandId)

// After:
await reg.keybindings.setOverride(commandId, chord)
await reg.keybindings.clearOverride(commandId)
```

Remove the `OverrideStorage` specifier from any import that only referenced it for the now-deleted storage object.

---

### Step 6 — Update `KeybindingRegistry.test.ts`

**File:** `shell/src/registry/KeybindingRegistry.test.ts`

Add a `freshRegistryWithStorage` helper:

```ts
function freshRegistryWithStorage(initial: Record<string, string> = {}) {
  const storage = memoryStorage(initial)
  const reg = new KeybindingRegistry()
  reg.bindStorage(storage)
  return { reg, storage }
}
```

Update all tests that passed `storage` as a method argument to use `bindStorage` instead and drop the argument from the call sites.

The two-session round-trip test keeps a shared `memoryStorage` instance but now binds it to each session's registry with `bindStorage`.

---

### Step 7 — Verify no orphaned references remain

```sh
grep -rn "keybindingOverrideStorage\|OVERRIDES_STORAGE_KEY" shell/src/plugins/
```

Expected: zero matches. `OverrideStorage` should appear only in `registry/`.

---

## Interface Changes Summary

| Location | Before | After |
|---|---|---|
| `KeybindingRegistry` | `loadOverrides(storage)` | `bindStorage(storage)` + `loadOverrides()` |
| `KeybindingRegistry` | `setOverride(storage, cmdId, chord)` | `setOverride(cmdId, chord)` |
| `KeybindingRegistry` | `clearOverride(storage, cmdId)` | `clearOverride(cmdId)` |
| `SettingsPanelView` | owns storage, passes it on every call | calls `setOverride`/`clearOverride` without storage arg |
| `settings/index.ts` | calls `loadOverrides` at activate via internal cast | no keybinding concern |
| `main.tsx` | no keybinding bootstrap | `bindStorage` + `loadOverrides` before plugin load |

## Critical Files

- `shell/src/registry/KeybindingRegistry.ts`
- `shell/src/registry/keybindingOverrideStorage.ts` *(new)*
- `shell/src/main.tsx`
- `shell/src/plugins/core/settings/SettingsPanelView.tsx`
- `shell/src/plugins/core/settings/index.ts`
- `shell/src/registry/KeybindingRegistry.test.ts`
