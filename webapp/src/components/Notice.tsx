import { useEffect } from "react";

interface Props {
  message: string | null;
  onDismiss: () => void;
}

/**
 * A single line of feedback, shown after an action that can fail.
 *
 * It sits at the bottom of the window so it never covers the panel the
 * action came from, and it clears itself so a stale message cannot linger.
 */
export function Notice({ message, onDismiss }: Props) {
  useEffect(() => {
    if (!message) return;
    const timer = window.setTimeout(onDismiss, 6000);
    return () => window.clearTimeout(timer);
  }, [message, onDismiss]);

  if (!message) return null;

  return (
    <div className="notice" role="status" aria-live="polite">
      <span>{message}</span>
      <button type="button" onClick={onDismiss} aria-label="Dismiss message">
        ✕
      </button>
    </div>
  );
}
