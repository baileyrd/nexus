// scripts/migrate-shell-state.ts
//
// WI-14 — one-shot migration of persisted state from the legacy `app/`
// shell to the new `shell/` shell.
//
// Reads `<input-dir>/layout-state.json` (the legacy
// `crates/nexus-app/src/persistence.rs` `LayoutPersistence` blob) and
// writes `<output-dir>/shell-state.json` (the new
// `shell/src-tauri/src/persistence.rs` `ShellState` blob), preserving
// what the new shell knows how to consume today and dropping fields it
// doesn't yet model.
//
// Usage:
//   tsx scripts/migrate-shell-state.ts <input-dir> <output-dir>
//
//     <input-dir>  : directory holding the legacy `layout-state.json`
//                    (typically Tauri's `app_config_dir()` for the
//                    legacy app — e.g. `~/.config/com.nexus.app`).
//     <output-dir> : directory where the new `shell-state.json` will
//                    be written (typically Tauri's `app_config_dir()`
//                    for the new shell). Created if missing.
//
// The function `migrate()` is exported as a pure function so the test
// can exercise it without touching the filesystem.
//
// Constraints (per WI-14):
//   - No new top-level deps. node:fs / node:path only.
//   - Idempotent: running twice on already-migrated state is a no-op.
//   - Graceful on empty input: `migrate({})` returns the new shell's
//     default empty state.
//
// Mapping (legacy → new):
//   lastForgePath        → lastForgePath        (1:1)
//   recentForgePaths     → recentForgePaths     (1:1, capped at 8)
//   version              → version              (forced to 1; new
//                                                shell uses its own
//                                                schema versioning)
//   lastPresetId         → DROPPED (new shell has no preset concept)
//   layouts              → DROPPED (new shell uses plugin-first layout,
//                                   not per-preset side-panel toggles)
//   forgeState           → DROPPED (per-forge tabs / expanded paths /
//                                   panes will move to per-plugin
//                                   storage as plugins migrate off
//                                   localStorage; not persisted in
//                                   shell-state.json today)

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'

// ── Legacy shape ──────────────────────────────────────────────────────────────
// Mirrors `LayoutPersistence` in `crates/nexus-app/src/persistence.rs`,
// camelCase per `#[serde(rename_all = "camelCase")]`. Every field is
// optional so we can ingest partial / corrupt files defensively.

export interface LegacyPersistedTab {
  relpath: string
  pinned?: boolean
}

export interface LegacyPersistedPaneState {
  tabs?: LegacyPersistedTab[]
  activeRelpath?: string | null
}

export interface LegacyForgeUiState {
  expandedPaths?: string[]
  openFile?: string | null
  panes?: Record<string, LegacyPersistedPaneState>
}

export interface LegacyPersistedLayoutState {
  leftSidePanelCollapsed?: boolean
  rightSidePanelCollapsed?: boolean
  leftActivePanelId?: string | null
  rightActivePanelId?: string | null
}

export interface LegacyShellState {
  version?: number
  lastPresetId?: string | null
  lastForgePath?: string | null
  recentForgePaths?: string[]
  layouts?: Record<string, LegacyPersistedLayoutState>
  forgeState?: Record<string, LegacyForgeUiState>
}

// ── New shape ─────────────────────────────────────────────────────────────────
// Mirrors `ShellState` in `shell/src-tauri/src/persistence.rs`. Source
// of truth is the Rust struct; this interface must stay in sync.

export interface NewShellState {
  version: number
  lastForgePath: string | null
  recentForgePaths: string[]
}

const NEW_SHELL_VERSION = 1
const MAX_RECENT_FORGES = 8

const LEGACY_FILE_NAME = 'layout-state.json'
const NEW_FILE_NAME = 'shell-state.json'

/** Default empty state matching `ShellState::default()` in Rust. */
export function emptyNewShellState(): NewShellState {
  return {
    version: NEW_SHELL_VERSION,
    lastForgePath: null,
    recentForgePaths: [],
  }
}

/**
 * Pure migration: legacy blob → new blob. Safe to call with `{}` (or
 * any partial input); always returns a fully-populated NewShellState.
 *
 * Dedupes `recentForgePaths` and re-applies the new shell's cap so a
 * legacy file written under a different cap (or hand-edited) lands in
 * a normal-looking state.
 */
export function migrate(input: LegacyShellState | null | undefined): NewShellState {
  const src = input ?? {}

  const lastForgePath =
    typeof src.lastForgePath === 'string' && src.lastForgePath.length > 0
      ? src.lastForgePath
      : null

  // Dedupe (preserving newest-first order) and cap. The legacy writer
  // already promotes lastForgePath to the front of recentForgePaths,
  // so we don't re-prepend it here — that would duplicate it.
  const seen = new Set<string>()
  const recents: string[] = []
  for (const raw of src.recentForgePaths ?? []) {
    if (typeof raw !== 'string' || raw.length === 0) continue
    if (seen.has(raw)) continue
    seen.add(raw)
    recents.push(raw)
    if (recents.length >= MAX_RECENT_FORGES) break
  }

  return {
    version: NEW_SHELL_VERSION,
    lastForgePath,
    recentForgePaths: recents,
  }
}

// ── CLI entry ─────────────────────────────────────────────────────────────────

interface RunResult {
  status: 'migrated' | 'no-input' | 'idempotent'
  output: NewShellState
  outputPath: string
}

/**
 * Read the legacy file from `inputDir`, migrate, and write the result
 * into `outputDir`. If the legacy file is absent, writes the new
 * shell's empty default state and returns `status: 'no-input'`. If the
 * resulting blob already matches what's on disk in `outputDir`, no
 * write happens and the call returns `status: 'idempotent'`.
 */
export function run(inputDir: string, outputDir: string): RunResult {
  const legacyPath = join(inputDir, LEGACY_FILE_NAME)
  const outputPath = join(outputDir, NEW_FILE_NAME)

  let legacy: LegacyShellState = {}
  let hadInput = false
  if (existsSync(legacyPath)) {
    try {
      const raw = readFileSync(legacyPath, 'utf8')
      const parsed = JSON.parse(raw) as unknown
      if (parsed && typeof parsed === 'object') {
        legacy = parsed as LegacyShellState
        hadInput = true
      }
    } catch (err) {
      // Match the Rust loader's policy: corrupt input is non-fatal,
      // we fall back to an empty migration.
      const message = err instanceof Error ? err.message : String(err)
      console.warn(`[migrate-shell-state] could not parse ${legacyPath}: ${message} — using defaults`)
    }
  }

  const next = migrate(legacy)

  // Idempotency: if the destination already holds the same blob,
  // skip the write so reruns are observably no-ops.
  if (existsSync(outputPath)) {
    try {
      const existing = JSON.parse(readFileSync(outputPath, 'utf8')) as NewShellState
      if (
        existing.version === next.version &&
        existing.lastForgePath === next.lastForgePath &&
        Array.isArray(existing.recentForgePaths) &&
        existing.recentForgePaths.length === next.recentForgePaths.length &&
        existing.recentForgePaths.every((p, i) => p === next.recentForgePaths[i])
      ) {
        return { status: 'idempotent', output: next, outputPath }
      }
    } catch {
      // Fallthrough: existing file unreadable, just overwrite.
    }
  }

  if (!existsSync(outputDir)) {
    mkdirSync(outputDir, { recursive: true })
  }
  writeFileSync(outputPath, `${JSON.stringify(next, null, 2)}\n`, 'utf8')

  return { status: hadInput ? 'migrated' : 'no-input', output: next, outputPath }
}

// CLI wrapper. Only fires when invoked directly via `tsx` / `node`;
// importing this module (e.g. from the test) leaves it inert.
//
// The `import.meta.url === ...` check is the standard ESM equivalent
// of `if __name__ == "__main__"`. Path-equality dance handles the
// `file://` prefix on `import.meta.url`.
const invokedDirectly = (() => {
  try {
    const entry = process.argv[1]
    if (!entry) return false
    return import.meta.url === `file://${entry}` ||
      import.meta.url.endsWith(entry.replace(/\\/g, '/'))
  } catch {
    return false
  }
})()

if (invokedDirectly) {
  const [, , inputDir, outputDir] = process.argv
  if (!inputDir || !outputDir) {
    console.error('Usage: tsx scripts/migrate-shell-state.ts <input-dir> <output-dir>')
    process.exit(2)
  }
  // Absolute-ize so error messages are unambiguous.
  const inAbs = inputDir
  const outAbs = outputDir
  const result = run(inAbs, outAbs)
  switch (result.status) {
    case 'migrated':
      console.log(`migrated → ${result.outputPath}`)
      break
    case 'no-input':
      console.log(`no legacy file in ${inputDir}; wrote empty default → ${result.outputPath}`)
      break
    case 'idempotent':
      console.log(`already up to date → ${result.outputPath}`)
      break
  }
  // Suppress unused-import warning under strict tsc: dirname is exposed
  // for callers that want to compose this with their own pathing.
  void dirname
}
