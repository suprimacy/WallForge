import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { ExperimentalStatus } from "../types";
import { formatCountdown, useTokenClock } from "../useTokenClock";

interface Props {
  status: ExperimentalStatus | null;
  onChange: (status: ExperimentalStatus) => void;
  onClose: () => void;
}

const KEY_PAGE = "https://manifesthub2.filegear-sg.me";

export function ExperimentalDialog({ status, onChange, onClose }: Props) {
  const [draft, setDraft] = useState(status?.tokenStatus.token ?? "");
  const [reveal, setReveal] = useState(false);
  const [message, setMessageText] = useState("");
  const [tone, setTone] = useState("");
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const say = (text: string, nextTone = "") => {
    setMessageText(text);
    setTone(nextTone);
  };

  const saved = status?.tokenStatus.token ?? "";
  const enabled = status?.enabled ?? false;
  const remaining = useTokenClock(status?.tokenStatus ?? null);
  const dirty = draft.trim() !== saved;

  useEffect(() => {
    setDraft(saved);
  }, [saved]);

  useEffect(() => {
    inputRef.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const badge = !draft.trim()
    ? { text: "No key — generate one, then paste it here.", tone: "warn" }
    : dirty
      ? { text: "Not saved yet — treated as valid until you save.", tone: "warn" }
      : remaining > 0
        ? { text: `Valid for ${formatCountdown(remaining)}`, tone: "success" }
        : { text: "Expired — generate a new key.", tone: "error" };

  const save = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const result = await api.tokenSave(draft);
      onChange({ enabled, tokenStatus: result.status });
      setDraft(result.status.token);
      if (result.warning) {
        say(result.warning, "warn");
      } else if (!result.status.token) {
        say("Key cleared.", "success");
      } else {
        say("Key saved. It is valid for 24 hours from when it was issued.", "success");
      }
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not save that key", "error");
    } finally {
      setBusy(false);
    }
  };

  const clear = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const result = await api.tokenClear();
      onChange({ enabled, tokenStatus: result.status });
      setDraft("");
      say("Key removed from this PC.", "success");
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not clear that key", "error");
    } finally {
      setBusy(false);
    }
  };

  const toggle = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await api.experimentalSetEnabled(!enabled);
      onChange(await api.experimentalStatus());
      say(
        !enabled
          ? "On. Downloads try a ManifestHub manifest before your Steam account."
          : "Off. Downloads use your Steam account again.",
        "success",
      );
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not change that setting", "error");
    } finally {
      setBusy(false);
    }
  };

  const openKeyPage = async () => {
    try {
      const result = await api.tokenOpenKeyPage();
      if (!result.success) say(result.message || "Could not open the key page.", "error");
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not open the key page", "error");
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
              <h2 id="experimental-dialog-title">Download without a Steam account</h2>
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
              <p className="field-label">Use ManifestHub manifests</p>
              <p className="hint">
                Try a public manifest first; fall back to Steam when one is missing.
              </p>
            </div>
            <button
              type="button"
              className={`switch${enabled ? " on" : ""}`}
              data-experimental-toggle
              role="switch"
              aria-checked={enabled}
              aria-label="Use ManifestHub manifests"
              onClick={() => void toggle()}
              disabled={busy}
            >
              <span className="switch-thumb" />
            </button>
          </div>

          <div className="dialog-section">
            <p className="hint">
              A free ManifestHub key lasts 24 hours. Get one at{" "}
              <code>{KEY_PAGE}</code>, then paste it here. The key is kept on this
              PC and never sent anywhere except ManifestHub.
            </p>
            <label className="field-label" htmlFor="token-input">
              ManifestHub API key
            </label>
            <div className="token-field">
              <input
                id="token-input"
                className="input token-input"
                type={reveal ? "text" : "password"}
                ref={inputRef}
                autoComplete="off"
                spellCheck={false}
                placeholder="Paste the 76-character hex key"
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && dirty) void save();
                }}
              />
              <button
                type="button"
                className="token-reveal"
                data-token-reveal
                aria-label={reveal ? "Hide key" : "Show key"}
                aria-pressed={reveal}
                title={reveal ? "Hide key" : "Show key"}
                onClick={() => setReveal((value) => !value)}
              >
                {reveal ? (
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.8"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    aria-hidden="true"
                    focusable="false"
                  >
                    <path d="M3 3l18 18" />
                    <path d="M10.6 10.6a2 2 0 0 0 2.8 2.8" />
                    <path d="M9.9 5.2A10.6 10.6 0 0 1 12 5c6.4 0 10 7 10 7a17 17 0 0 1-2.2 3.2" />
                    <path d="M6.5 6.5C4 8.1 2 12 2 12s3.6 7 10 7a10.7 10.7 0 0 0 4.3-.9" />
                  </svg>
                ) : (
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.8"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    aria-hidden="true"
                    focusable="false"
                  >
                    <path d="M2 12s3.6-6 10-6 10 6 10 6-3.6 6-10 6-10-6-10-6Z" />
                    <circle cx="12" cy="12" r="3" />
                  </svg>
                )}
              </button>
            </div>
            <div className="token-actions">
              <button type="button" className="secondary" onClick={() => void openKeyPage()}>
                Open key page
              </button>
              <button type="button" className="secondary" onClick={() => void save()} disabled={!dirty || busy}>
                {busy ? "Saving..." : "Save key"}
              </button>
              <button type="button" className="ghost" onClick={() => void clear()} disabled={!saved || busy}>
                Clear
              </button>
              <span className={`token-badge ${badge.tone}`}>{badge.text}</span>
            </div>
            {status?.tokenStatus.savedAt ? (
              <p className="token-meta">
                Saved {new Date(status.tokenStatus.savedAt * 1000).toLocaleString()} ·
                expires {new Date(status.tokenStatus.expiresAt * 1000).toLocaleString()}
              </p>
            ) : null}
          </div>

          <p className="dialog-message hint">
            Experimental: ManifestHub is a third-party service and its cache may not
            have every version. A missing manifest, an expired key, or a failed fetch
            falls back to your signed-in Steam account. A rejected key (403) stops
            the queue so the rest do not fail one by one.
          </p>

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
