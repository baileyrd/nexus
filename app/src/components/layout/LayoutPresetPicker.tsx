import { useEffect } from "react";
import { useLayoutStore } from "../../stores/layout";

export function LayoutPresetPicker() {
  const layout = useLayoutStore((s) => s.layout);
  const presets = useLayoutStore((s) => s.presets);
  const loadPreset = useLayoutStore((s) => s.loadPreset);
  const loadPresetList = useLayoutStore((s) => s.loadPresetList);

  useEffect(() => {
    if (presets.length === 0) {
      loadPresetList();
    }
  }, [presets.length, loadPresetList]);

  return (
    <div className="preset-picker" role="radiogroup" aria-label="Layout preset">
      {presets.map((p) => {
        const active = layout?.name === p.name;
        return (
          <button
            key={p.id}
            type="button"
            role="radio"
            aria-checked={active}
            className={active ? "active" : ""}
            title={p.description ?? undefined}
            onClick={() => loadPreset(p.id)}
          >
            {p.name}
          </button>
        );
      })}
    </div>
  );
}
