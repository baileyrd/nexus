import { useLayoutStore } from "../../stores/layout";
import type { LayoutPresetName } from "../../ipc/layout";

const PRESETS: { value: LayoutPresetName; label: string }[] = [
  { value: "writing", label: "Writing" },
  { value: "reviewing", label: "Reviewing" },
  { value: "coding", label: "Coding" },
];

export function LayoutPresetPicker() {
  const layout = useLayoutStore((s) => s.layout);
  const loadPreset = useLayoutStore((s) => s.loadPreset);

  return (
    <div className="preset-picker" role="radiogroup" aria-label="Layout preset">
      {PRESETS.map((p) => {
        const active = layout?.name.toLowerCase() === p.value;
        return (
          <button
            key={p.value}
            type="button"
            role="radio"
            aria-checked={active}
            className={active ? "active" : ""}
            onClick={() => loadPreset(p.value)}
          >
            {p.label}
          </button>
        );
      })}
    </div>
  );
}
