import { useEffect } from "react";
import { ThemePicker } from "./components/ThemePicker";
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
        <h1>Nexus</h1>
        <p className="tagline">theming preview · PRD 07 scaffold</p>
      </header>
      <main>
        <ThemePicker />
      </main>
    </div>
  );
}
