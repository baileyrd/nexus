import { useEffect } from "react";
import { useThemeStore } from "../stores/theme";

export function ThemePicker() {
  const themes = useThemeStore((s) => s.themes);
  const currentThemeId = useThemeStore((s) => s.currentThemeId);
  const loadAll = useThemeStore((s) => s.loadAll);
  const applyTheme = useThemeStore((s) => s.applyTheme);
  const loading = useThemeStore((s) => s.loading);
  const error = useThemeStore((s) => s.error);

  useEffect(() => {
    loadAll();
  }, [loadAll]);

  if (error) {
    return (
      <section className="theme-picker">
        <h2>Theme</h2>
        <p className="error">Failed to load themes: {error}</p>
      </section>
    );
  }

  return (
    <section className="theme-picker">
      <header>
        <h2>Theme</h2>
        {loading && <span className="hint">working…</span>}
      </header>
      <ul>
        {themes.map((theme) => (
          <li key={theme.id}>
            <button
              type="button"
              className={theme.id === currentThemeId ? "active" : ""}
              onClick={() => applyTheme(theme.id)}
            >
              <span className="name">{theme.name}</span>
              <span className="category" data-category={theme.category}>
                {theme.category}
              </span>
              <span className="desc">{theme.description}</span>
              {theme.builtin && <span className="badge">built-in</span>}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
