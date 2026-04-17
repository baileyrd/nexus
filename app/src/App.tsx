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
import { getPlatformInfo } from "./ipc/platform";
import type { PluginEvent } from "./plugins/events";
import {
  activateByUriScheme,
  refreshActivationTable,
  stopAllScriptPlugins,
} from "./plugins/scriptRuntime";

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

  // Publish the platform descriptor as a data attribute on <html> so
  // platform-scoped CSS (§11: vibrancy/Mica-friendly backgrounds, CSD
  // titlebar spacing) can take effect without shipping separate
  // stylesheets. Chrome-effect plugins read the attribute to decide
  // whether to request a native vibrancy layer.
  useEffect(() => {
    let cancelled = false;
    getPlatformInfo()
      .then((info) => {
        if (cancelled) return;
        const root = document.documentElement;
        root.dataset.platform = info.os;
        root.dataset.arch = info.arch;
        if (info.supportsVibrancy) root.dataset.supportsVibrancy = "true";
      })
      .catch(() => {
        // Tauri bridge unavailable (e.g. vite preview outside the app);
        // leave attributes unset so CSS falls back to the cross-platform rules.
      });
    return () => {
      cancelled = true;
    };
  }, []);

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
  // Before dispatch, activate any script plugin whose manifest declared
  // `on_uri_scheme` for the incoming scheme (UI F-3.2.1).
  useEffect(() => {
    const unlistenPromise = listen<string>(
      "nexus:url-opened",
      (event) => {
        activateByUriScheme(event.payload);
        contributions.dispatchUri(event.payload);
      },
    );
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  // Pull the activation table once at boot so content-type / URI-scheme
  // triggers know which plugins to lazy-load (UI F-3.2.1). Refreshed by
  // the contribution bridge after every hot-reload.
  useEffect(() => {
    void refreshActivationTable();
  }, []);

  // On window close / WebView teardown give every loaded script plugin a
  // chance to run `onStop` and flush its disposable store. The `beforeunload`
  // handler fires synchronously, so we kick off the stop and let it settle
  // on a best-effort basis (per-plugin budget enforced in stopAllScriptPlugins).
  useEffect(() => {
    function onBeforeUnload() {
      void stopAllScriptPlugins();
    }
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
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
