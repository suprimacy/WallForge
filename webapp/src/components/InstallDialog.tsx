import { useEffect, useState } from "react";
import { api } from "../api";
import type { InstallInfo } from "../types";

interface Props {
  info: InstallInfo | null;
  onClose: () => void;
  onChanged: () => void;
}

export function InstallDialog({ info, onClose, onChanged }: Props) {
  const [mode, setMode] = useState<"auto" | "manual">("auto");
  const [path, setPath] = useState("");
  const [message, setMessageText] = useState("");
  const [tone, setTone] = useState("");
  const say = (text: string, nextTone = "") => {
    setMessageText(text);
    setTone(nextTone);
  };
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (info?.install_dir) setPath(info.install_dir);
    else if (info?.auto_install_dir) setPath(info.auto_install_dir);
    setMode(info?.install_dir ? "manual" : "auto");
  }, [info]);

  const redetect = async () => {
    setBusy(true);
    try {
      // info() re-runs the detection scan, it does not open the picker.
      const fresh = await api.info();
      if (fresh.auto_install_dir) {
        say(`Found: ${fresh.auto_install_dir}`, "success");
      } else {
        say("Not found automatically. Pick the folder in Manual.", "warn");
      }
      onChanged();
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not re-detect", "error");
    } finally {
      setBusy(false);
    }
  };

  const browse = async () => {
    setBusy(true);
    try {
      const result = await api.browseInstallDir();
      if (!result.success) {
        say(result.message || "Windows would not open the folder picker.", "error");
        return;
      }
      // The picked folder travels with the result; the saved install dir
      // does not change until Save.
      if (result.cancelled || !result.path) {
        // The user closed the picker without choosing.
        return;
      }
      setPath(result.path);
      say(`Folder selected: ${result.path}`, "success");
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not browse", "error");
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    try {
      const result = await api.setInstallDir(path);
      if (!result.success) throw new Error(result.message || "Could not save that folder");
      say(result.message || "Folder saved.", "success");
      onChanged();
      onClose();
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not save that folder", "error");
    } finally {
      setBusy(false);
    }
  };

  const useAuto = async () => {
    try {
      await api.resetInstallDir();
      say("Using the folder WallForge detected.", "success");
      onChanged();
      onClose();
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not reset", "error");
    }
  };

  const candidates = info?.auto_candidates ?? [];

  return (
    <div
      className="dialog-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="install-dialog-title"
      >
        <div className="dialog-inner">
          <div className="dialog-head">
            <div>
              <p className="eyebrow">Wallpaper Engine folder</p>
              <h2 id="install-dialog-title">Where is Wallpaper Engine installed?</h2>
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

          <div className="segmented" role="radiogroup" aria-label="Detection mode">
            <button
              type="button"
              className={`segment${mode === "auto" ? " active" : ""}`}
              data-install-mode="auto"
              role="radio"
              aria-checked={mode === "auto"}
              onClick={() => setMode("auto")}
            >
              Automatic
            </button>
            <button
              type="button"
              className={`segment${mode === "manual" ? " active" : ""}`}
              data-install-mode="manual"
              role="radio"
              aria-checked={mode === "manual"}
              onClick={() => setMode("manual")}
            >
              Manual
            </button>
          </div>

          {mode === "auto" ? (
            <div className="dialog-section">
              <p className="hint">
                Found automatically from the Wallpaper Engine registry key or Steam.
              </p>
              <div className="path-row">
                <output className="path-output">
                  {info?.auto_install_dir || "Not detected"}
                </output>
                <button type="button" className="secondary" onClick={redetect}>
                  Re-detect
                </button>
              </div>
              {candidates.length ? (
                <p id="install-auto-list">
                  {candidates.map((candidate) => `Also checked: ${candidate}`).join("\n")}
                </p>
              ) : null}
            </div>
          ) : (
            <div className="dialog-section">
              <p className="hint">
                Pick the folder that contains <code>projects</code> or{" "}
                <code>wallpaper32.exe</code>. The <code>projects</code> and{" "}
                <code>projects\myprojects</code> folders work too.
              </p>
              <div className="path-row">
                <input
                  id="install-path"
                  className="input"
                  type="text"
                  placeholder="C:\Program Files (x86)\Steam\steamapps\common\Wallpaper Engine"
                  autoComplete="off"
                  spellCheck={false}
                  value={path}
                  onChange={(event) => setPath(event.target.value)}
                />
                <button type="button" className="secondary" onClick={browse}>
                  Browse…
                </button>
              </div>
            </div>
          )}

          <p className={`dialog-message${tone ? ` ${tone}` : ""}`} role="status">
            {message}
          </p>

          <div className="dialog-actions">
            <button type="button" className="ghost" onClick={onClose}>
              Cancel
            </button>
            {info?.install_dir ? (
              <button type="button" className="secondary" onClick={useAuto}>
                Use automatic
              </button>
            ) : null}
            <button
              type="button"
              className="primary"
              onClick={save}
              disabled={busy || !path}
            >
              Save folder
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

