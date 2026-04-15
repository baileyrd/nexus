import { useEffect } from "react";
import { ModeToggle } from "./components/ModeToggle";
import { CommandPalette } from "./components/palette/CommandPalette";
import { ThemePicker } from "./components/ThemePicker";
import { WorkspaceView } from "./components/layout/WorkspaceView";
import { useThemeStore } from "./stores/theme";

export default function App() {
  const applyTheme = useThemeStore((s) => s.applyTheme);
  const currentThemeId = useThemeStore((s) => s.currentThemeId);

  // Pick the built-in light theme on first mount so the picker reflects an
  // "active" selection immediately. The user can switch freely from there.
  useEffect(() => {
    if (!currentThemeId) {
      applyTheme("nexus-light");
    }
  }, [applyTheme, currentThemeId]);

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
        <ThemePicker />
      </main>
      <CommandPalette />
    </div>
  );
}
