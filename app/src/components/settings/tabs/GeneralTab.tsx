import { ThemePicker } from "../../ThemePicker";
import {
  useEditorPrefsStore,
  type KeybindingMode,
  type ViewMode,
} from "../../../stores/editorPrefs";

/**
 * Home for broad app preferences: theme + editor mode preferences.
 */
export function GeneralTab() {
  return (
    <div className="settings-tab">
      <ThemePicker />
      <EditorPreferences />
    </div>
  );
}

function EditorPreferences() {
  const keybindingMode = useEditorPrefsStore((s) => s.keybindingMode);
  const setKeybindingMode = useEditorPrefsStore((s) => s.setKeybindingMode);
  const viewMode = useEditorPrefsStore((s) => s.viewMode);
  const setViewMode = useEditorPrefsStore((s) => s.setViewMode);

  return (
    <section className="settings-section">
      <h3>Editor</h3>
      <div className="settings-row">
        <label htmlFor="nx-keybinding-mode">Keybindings</label>
        <select
          id="nx-keybinding-mode"
          className="plugin-settings-input"
          value={keybindingMode}
          onChange={(e) => setKeybindingMode(e.target.value as KeybindingMode)}
        >
          <option value="default">Default</option>
          <option value="vim">Vim</option>
          <option value="emacs">Emacs</option>
        </select>
      </div>
      <div className="settings-row">
        <label htmlFor="nx-view-mode">View mode</label>
        <select
          id="nx-view-mode"
          className="plugin-settings-input"
          value={viewMode}
          onChange={(e) => setViewMode(e.target.value as ViewMode)}
        >
          <option value="live">Live preview</option>
          <option value="source">Source</option>
          <option value="reading">Reading</option>
        </select>
      </div>
    </section>
  );
}
