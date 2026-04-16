import { X } from "lucide-react";
import { useToastStore, type ToastLevel } from "../stores/toast";

function levelLabel(level: ToastLevel): string {
  if (level === "error") return "Error";
  if (level === "warn") return "Warning";
  return "Info";
}

/** Fixed-position overlay that renders active plugin/host notifications. */
export function ToastOverlay() {
  const toasts = useToastStore((s) => s.toasts);
  const remove = useToastStore((s) => s.remove);

  if (toasts.length === 0) return null;

  return (
    <div className="toast-overlay" role="region" aria-label="Notifications" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast toast-${t.level}`} role="alert">
          <span className="toast-badge" aria-label={levelLabel(t.level)} />
          <span className="toast-message">{t.message}</span>
          {t.source && (
            <span className="toast-source" title={t.source}>
              {t.source.split(".").pop()}
            </span>
          )}
          <button
            type="button"
            className="toast-dismiss"
            aria-label="Dismiss notification"
            onClick={() => remove(t.id)}
          >
            <X size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}
