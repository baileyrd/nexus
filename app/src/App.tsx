import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { ModeToggle } from "./components/ModeToggle";
import { CommandPalette } from "./components/palette/CommandPalette";
import { SettingsModal } from "./components/settings/SettingsModal";
import { ToastOverlay } from "./components/ToastOverlay";
import { WorkspaceView } from "./components/layout/WorkspaceView";
import { KeybindingDispatcher } from "./keybindings/KeybindingDispatcher";
import { contributions } from "./contributions";
import { useForgeStore } from "./stores/forge";
import { useLayoutStore } from "./stores/layout";
import { useThemeStore } from "./stores/theme";
import { useToastStore, type ToastLevel } from "./stores/toast";
import { THEME_CHANGED_EVENT } from "./ipc/theme";
import type { ThemeConfig } from "./ipc/theme";
import type { PluginEvent } from "./plugins/events";

export default function App() {
  const applyTheme = useThemeStore((s) => s.applyTheme);
  const currentThemeId = useThemeStore((s) => s.currentThemeId);
  const syncFromEngine = useThemeStore((s) => s.syncFromEngine);
  const loadForge = useForgeStore((s) => s.load);
  const bumpFsVersion = useForgeStore((s) => s.bumpFsVersion);
  const hydrateForge = useForgeStore((s) => s.hydrate);
  const forgeRoot = useForgeStore((s) => s.info?.root);
  const layoutPersistenceLoaded = useLayoutStore((s) => s.persistence !== null);
  const addToast = useToastStore((s) => s.add);

  // Pick the built-in light theme on first mount so the picker reflects an
  // "active" selection immediately. The user can switch freely from there.
  useEffect(() => {
    if (!currentThemeId) {
      applyTheme("nexus-light");
    }
  }, [applyTheme, currentThemeId]);

  useEffect(() => {
    loadForge();
  }, [loadForge]);

  useEffect(() => {
    const unlistenPromise = listen("forge:fs-changed", () => {
      bumpFsVersion();
    });
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [bumpFsVersion]);

  // Listen for plugin-driven theme mutations forwarded from the kernel
  // event bus (`com.nexus.theme.changed` → `theme:changed`) and mirror
  // them into the store. Self-driven mutations are elided by
  // `syncFromEngine`'s equality check, so we don't loop.
  useEffect(() => {
    const unlistenPromise = listen<ThemeConfig>(
      THEME_CHANGED_EVENT,
      (event) => {
        void syncFromEngine(event.payload);
      },
    );
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [syncFromEngine]);

  // Restore expanded paths + last-open file when both the forge and the
  // layout persistence are ready. Re-runs whenever the active forge
  // changes so switching forges restores that forge's state.
  useEffect(() => {
    if (forgeRoot && layoutPersistenceLoaded) {
      hydrateForge();
    }
  }, [forgeRoot, layoutPersistenceLoaded, hydrateForge]);

  // Dispatch incoming deep-link URLs to registered URI handlers.
  // The Rust backend emits "nexus:url-opened" (via `dispatch_uri` Tauri
  // command or the tauri-plugin-deep-link bridge) with the full URL string.
  useEffect(() => {
    const unlistenPromise = listen<string>(
      "nexus:url-opened",
      (event) => {
        contributions.dispatchUri(event.payload);
      },
    );
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  // Surface plugin notifications (topic = "ui.notification") as toasts.
  useEffect(() => {
    const unlistenPromise = listen<PluginEvent<{ level?: string; message?: string }>>(
      "plugin:event",
      (event) => {
        const { topic, plugin_id, payload } = event.payload;
        if (topic !== "ui.notification") return;
        const level = (["info", "warn", "error"].includes(payload?.level ?? "")
          ? payload.level
          : "info") as ToastLevel;
        const message = typeof payload?.message === "string" ? payload.message : String(payload);
        addToast({ level, message, source: plugin_id });
      },
    );
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [addToast]);

  return (
    <div className="app">
      <header className="app-header">
        <div className="app-title">
          <h1>Nexus</h1>
          <p className="tagline">theming preview · PRD 07 scaffold</p>
        </div>
        <ModeToggle />
      </header>
      <main>
        <WorkspaceView />
      </main>
      <CommandPalette />
      <SettingsModal />
      <ToastOverlay />
      <KeybindingDispatcher />
    </div>
  );
}
