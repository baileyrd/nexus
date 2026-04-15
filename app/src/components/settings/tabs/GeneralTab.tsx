import { ThemePicker } from "../../ThemePicker";

/**
 * Home for broad app preferences. Today: theme selection. Future:
 * language, default-open-behavior, startup-diagnostics, etc.
 */
export function GeneralTab() {
  return (
    <div className="settings-tab">
      <ThemePicker />
    </div>
  );
}
