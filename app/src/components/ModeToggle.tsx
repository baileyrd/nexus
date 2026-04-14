import { useThemeStore } from "../stores/theme";
import type { ThemeMode } from "../ipc/theme";

const MODES: { value: ThemeMode; label: string; icon: string }[] = [
  { value: "light", label: "Light", icon: "☀" },
  { value: "dark", label: "Dark", icon: "☾" },
  { value: "system", label: "System", icon: "⚙" },
];

export function ModeToggle() {
  const mode = useThemeStore((s) => s.mode);
  const setMode = useThemeStore((s) => s.setMode);

  return (
    <div
      className="mode-toggle"
      role="radiogroup"
      aria-label="Colour mode"
    >
      {MODES.map((m) => (
        <button
          key={m.value}
          type="button"
          role="radio"
          aria-checked={mode === m.value}
          className={mode === m.value ? "active" : ""}
          onClick={() => setMode(m.value)}
          title={m.label}
        >
          <span aria-hidden>{m.icon}</span>
          <span className="label">{m.label}</span>
        </button>
      ))}
    </div>
  );
}
