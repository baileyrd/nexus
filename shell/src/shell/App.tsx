// src/shell/App.tsx
import { useEffect, useRef, useState } from 'react'
import { useSlotStore } from '../registry/SlotRegistry'
import { usePaneModeStore } from '../stores/paneModeStore'
import { SlotSurface } from './slots/SlotSurface'
import { getRegistry } from '../host/shellRegistry'
import { contextKeyService, useContextKey } from '../host/ContextKeyService'
import { useWorkspaceStore as useNexusWorkspaceStore } from '../plugins/nexus/workspace/workspaceStore'
import { Workspace } from '../workspace/WorkspaceRenderer'
import { workspace as workspaceStore } from '../workspace/workspaceStore'
import {
  buildDefaultLayout,
  installAutoSave,
  loadWorkspace,
} from '../workspace'

export default function App() {
  const slots = useSlotStore(s => s.slots)
  const paneModeViewId = usePaneModeStore(s => s.activeViewId)
  const rootPath = useNexusWorkspaceStore(s => s.rootPath)
  // shellReady flips to true in main.tsx boot() AFTER every plugin has
  // activated — guarantees all viewRegistry.register(...) calls have run
  // before we hydrate. Without this gate, workspacePlugin (which publishes
  // rootPath) activates early, hydrate runs while filesPlugin/editorPlugin/
  // etc. haven't registered their view creators, and every saved leaf
  // falls back to the `empty` creator.
  const shellReady = useContextKey('shellReady') === true
  const [debugInfo, setDebugInfo] = useState<string>('')
  const [hydrated, setHydrated] = useState(false)
  const autoSaveStopRef = useRef<(() => void) | null>(null)
  const lastHydratedPathRef = useRef<string | null>(null)

  useEffect(() => {
    // Debug: log what's in each slot after mount
    const timer = setTimeout(() => {
      const reg = getRegistry()
      const info = [
        `Registry: ${reg ? 'loaded' : 'NULL'}`,
        `activityBar: ${slots.activityBar.length}`,
        `statusBarLeft: ${slots.statusBarLeft.length}`,
        `statusBarRight: ${slots.statusBarRight.length}`,
        `overlay: ${slots.overlay.length}`,
      ].join(' | ')
      console.info('[App] Slots:', info)
      setDebugInfo(info)
    }, 500)
    return () => clearTimeout(timer)
  }, [slots])

  // ── Workspace boot sequence ───────────────────────────────────────────────
  //
  // Plan: /home/baileyrd/projects/nexus/docs/leaf-migration-plan.md §Phase 6.
  //
  // Ordering constraint: every plugin's `viewRegistry.register(...)` call
  // must have run BEFORE `hydrate(json)` so `setViewState` can resolve
  // every saved leaf's creator. Plugin activation happens in main.tsx's
  // `boot()` and is NOT awaited before React mounts — activation races
  // the initial render. We key hydration off `rootPath` becoming
  // non-null: `nexus.workspace` only publishes a root after `boot_kernel`
  // resolves, and by that point every core plugin's `activate()` has
  // been awaited in `ExtensionHost.loadAll` (main.tsx line 158). So
  // rootPath!==null implies all view types are registered.
  //
  // When the user switches workspaces (rootPath changes), we re-run the
  // cycle: stop auto-save for the previous vault, load+hydrate the new
  // vault's workspace.json, restart auto-save.
  useEffect(() => {
    if (rootPath === null) {
      // Vault closed. Stop autosaving into the previous vault's path.
      if (autoSaveStopRef.current) {
        autoSaveStopRef.current()
        autoSaveStopRef.current = null
      }
      setHydrated(false)
      lastHydratedPathRef.current = null
      return
    }
    if (!shellReady) return
    if (lastHydratedPathRef.current === rootPath) return
    lastHydratedPathRef.current = rootPath

    let cancelled = false
    void (async () => {
      try {
        const saved = await loadWorkspace(rootPath)
        if (cancelled) return
        const json = saved ?? buildDefaultLayout()
        await workspaceStore.hydrate(json)
        if (cancelled) return
        // Replace any previous autosave subscription before installing new.
        if (autoSaveStopRef.current) autoSaveStopRef.current()
        autoSaveStopRef.current = installAutoSave(rootPath)
        setHydrated(true)
      } catch (err) {
        console.error('[App] workspace hydrate failed, falling back to default', err)
        if (cancelled) return
        const fallback = buildDefaultLayout()
        await workspaceStore.hydrate(fallback)
        if (!cancelled) {
          if (autoSaveStopRef.current) autoSaveStopRef.current()
          autoSaveStopRef.current = installAutoSave(rootPath)
          setHydrated(true)
        }
      }
    })()

    return () => {
      cancelled = true
      // Clear the ref so a StrictMode re-invocation (which runs the cleanup
      // before install/setHydrated have completed) retries instead of short-
      // circuiting on line 71. The cancelled flag prevents the aborted run
      // from racing the retry.
      if (lastHydratedPathRef.current === rootPath) {
        lastHydratedPathRef.current = null
      }
    }
  }, [rootPath, shellReady])

  // Unmount-time cleanup: dispose auto-save subscriptions.
  useEffect(() => {
    return () => {
      if (autoSaveStopRef.current) {
        autoSaveStopRef.current()
        autoSaveStopRef.current = null
      }
    }
  }, [])

  // Global keyboard dispatcher
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) return
      const reg = getRegistry()
      if (!reg) return
      const keys = contextKeyService.snapshot()
      const commandId = reg.keybindings.match(e, keys)
      if (commandId) {
        e.preventDefault()
        e.stopPropagation()
        reg.commands.execute(commandId)
      }
    }
    document.addEventListener('keydown', handler)
    return () => document.removeEventListener('keydown', handler)
  }, [])

  const totalSlots = Object.values(slots).reduce((sum, arr) => sum + arr.length, 0)

  // If nothing is in any slot yet, show a loading indicator
  if (totalSlots === 0) {
    return (
      <div style={{
        height: '100vh',
        background: '#1e1e1e',
        color: '#cccccc',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        fontFamily: 'system-ui',
        fontSize: '13px',
      }}>
        Loading plugins...
      </div>
    )
  }

  // Swallow debugInfo for now — surfaced via console.info above.
  void debugInfo

  return (
    <div className="shell-root">

      {/* Overlay */}
      <div className="shell-overlay">
        <SlotSurface entries={slots.overlay} />
      </div>

      {/* Workspace — Obsidian-faithful top-level container. Hosts the
          ribbon (.workspace-ribbon.mod-left) and the body columns
          (.workspace-split.mod-left-split / .mod-root / .mod-right-split)
          as direct flex siblings. */}
      <div className="workspace">

        {/* Activity bar — `.workspace-ribbon.mod-left` in Obsidian.
            Chrome slot — kept as SlotRegistry entry. */}
        <div className="workspace-ribbon mod-left">
          <SlotSurface entries={slots.activityBar} />
        </div>

        {(() => {
          // Pane-mode: one slot entry takes over the entire body region,
          // bypassing the leaf renderer. The activity bar stays visible
          // (sibling above); statusbar / overlay are rendered outside.
          const paneEntry = paneModeViewId
            ? slots.paneMode.find(e => e.id === paneModeViewId)
            : undefined

          if (paneModeViewId && !paneEntry) {
            console.warn(
              `[App] Pane-mode viewId "${paneModeViewId}" is set but no matching slot entry exists; falling through to workspace renderer.`,
            )
          }

          if (paneEntry) {
            return (
              <div className="shell-pane-mode">
                <SlotSurface entries={[paneEntry]} />
              </div>
            )
          }

          // Phase 6 (plan line 182): <Workspace> owns the entire body
          // region — sidebar + main editor + right panel. Replaces the
          // former SlotSurface renders for sidebar / editorArea /
          // panelArea / rightPanel. Those slots become dead once
          // plugins stop registering into them (Phase 7 cleanup).
          //
          // Until the workspace has hydrated we render an empty shell
          // so chrome (activity bar, status bar, overlay) is visible and
          // plugin activation can finish without a flash of the old
          // layout.
          return (
            <div className="workspace-main-region" style={{ flex: '1 1 auto', minWidth: 0, display: 'flex' }}>
              {hydrated ? <Workspace /> : null}
            </div>
          )
        })()}
      </div>

    </div>
  )
}
