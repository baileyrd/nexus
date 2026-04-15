import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { ModeToggle } from "./components/ModeToggle";
import { CommandPalette } from "./components/palette/CommandPalette";
import { SettingsModal } from "./components/settings/SettingsModal";
import { WorkspaceView } from "./components/layout/WorkspaceView";
import { useForgeStore } from "./stores/forge";
import { useLayoutStore } from "./stores/layout";
import { useThemeStore } from "./stores/theme";

export default function App() {
  const applyTheme = useThemeStore((s) => s.applyTheme);
  const currentThemeId = useThemeStore((s) => s.currentThemeId);
  const loadForge = useForgeStore((s) => s.load);
  const bumpFsVersion = useForgeStore((s) => s.bumpFsVersion);
  const hydrateForge = useForgeStore((s) => s.hydrate);
  const forgeRoot = useForgeStore((s) => s.info?.root);
  const layoutPersistenceLoaded = useLayoutStore((s) => s.persistence !== null);

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

  // Restore expanded paths + last-open file when both the forge and the
  // layout persistence are ready. Re-runs whenever the active forge
  // changes so switching forges restores that forge's state.
  useEffect(() => {
    if (forgeRoot && layoutPersistenceLoaded) {
      hydrateForge();
    }
  }, [forgeRoot, layoutPersistenceLoaded, hydrateForge]);

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
    </div>
  );
}
