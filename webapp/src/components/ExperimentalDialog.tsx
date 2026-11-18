import { useEffect, useState } from "react";
import { api } from "../api";
import type { ExperimentalStatus } from "../types";

interface Props {
  status: ExperimentalStatus | null;
  onChange: (status: ExperimentalStatus) => void;
  onClose: () => void;
}

export function ExperimentalDialog({ status, onChange, onClose }: Props) {
  const [message, setMessageText] = useState("");
  const [tone, setTone] = useState("");
  const [busy, setBusy] = useState(false);
  const enabled = status?.enabled ?? false;

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const toggle = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await api.experimentalSetEnabled(!enabled);
      onChange(await api.experimentalStatus());
      setMessageText(
        !enabled
          ? "On. Downloads try a public manifest before your Steam account."
          : "Off. Downloads use your Steam account again.",
      );
      setTone("success");
    } catch (err) {
      setMessageText(err instanceof Error ? err.message : "Could not change that setting");
      setTone("error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="dialog-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="dialog experimental-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="experimental-dialog-title"
      >
        <div className="dialog-inner">
          <div className="dialog-head">
            <div>
              <p className="eyebrow">Experimental</p>
              <h2 id="experimental-dialog-title">Download anonymously</h2>
            </div>
            <button type="button" className="icon-btn" aria-label="Close" onClick={onClose}>
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.9"
                strokeLinecap="round"
                aria-hidden="true"
                focusable="false"
              >
                <path d="M6 6l12 12M18 6L6 18" />
              </svg>
            </button>
          </div>

          <div className="experiment-switch-row">
            <div>
              <p className="field-label">Use anonymous manifests</p>
              <p className="hint">
                No Steam account needed. Falls back to Steam if a manifest is unavailable.
              </p>
            </div>
            <button
              type="button"
              className={`switch${enabled ? " on" : ""}`}
              data-experimental-toggle
              role="switch"
              aria-checked={enabled}
              aria-label="Use anonymous manifests"
              onClick={() => void toggle()}
              disabled={busy}
            >
              <span className="switch-thumb" />
            </button>
          </div>

          <div className="dialog-section">
            <p className="hint">
              WallForge asks Steam for the item's manifest ID, gets a short-lived code from a
              public service, and fetches the manifest from a Steam CDN node. The service sees
              your IP and the manifest ID only — no account, password or key. The manifest is
              cached in the wallpaper's own folder, so cached items never contact it again.
            </p>
            <p className="hint proxy-hint">
              To hide your IP from that one request, set the standard <code>HTTPS_PROXY</code>{" "}
              environment variable before starting WallForge. Wallpaper downloads always
              connect directly to Steam.
            </p>
          </div>

          <p className={`dialog-message${tone ? ` ${tone}` : ""}`} role="status">
            {message}
          </p>

          <div className="dialog-actions">
            <button type="button" className="ghost" onClick={onClose}>
              Close
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
