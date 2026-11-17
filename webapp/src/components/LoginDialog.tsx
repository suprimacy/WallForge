import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { AccountEntry, LoginStatus } from "../types";

interface Props {
  status: LoginStatus | null;
  onRefresh: () => void;
  onClose: () => void;
}

type LoginAction = "signin" | "store-password";

export function LoginDialog({ status, onRefresh, onClose }: Props) {
  const [method, setMethod] = useState<"password" | "qr">("password");
  const [action, setAction] = useState<LoginAction>("signin");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [message, setMessageText] = useState("");
  const [tone, setTone] = useState("");
  const say = (text: string, nextTone = "") => {
    setMessageText(text);
    setTone(nextTone);
  };
  const [busy, setBusy] = useState(false);
  const [accounts, setAccounts] = useState<AccountEntry[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [qrSrc, setQrSrc] = useState("");
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const passwordRef = useRef<HTMLInputElement | null>(null);

  const state = status?.status ?? "idle";

  useEffect(() => {
    if (!status) return;
    setAccounts(status.accounts ?? []);
    setActive(status.active ?? null);
    if (status.status === "error" && status.message) {
      say(status.message);
      setTone("error");
    }
  }, [status]);

  useEffect(() => {
    if (!status?.has_qr) {
      setQrSrc("");
      return;
    }
    let cancelled = false;
    void api.qrImage(status.qr_version).then((source) => {
      if (!cancelled) setQrSrc(source);
    });
    return () => {
      cancelled = true;
    };
  }, [status?.has_qr, status?.qr_version]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const useAccount = async (name: string, ready: boolean) => {
    try {
      const result = await api.selectAccount(name);
      if (!result.success) throw new Error(result.message || "Could not switch account");
      setAccounts(result.accounts ?? accounts);
      setActive(result.active ?? null);
      if (ready) {
        say(`Downloads will use ${result.active}.`, "success");
      } else {
        setUsername(result.active ?? name);
        setPassword("");
        setMethod("password");
        setAction("signin");
        say(
          `${result.active} has not signed in on this PC yet. Sign in once with the password or QR code below, then downloads will use it.`,
          "warn",
        );
        passwordRef.current?.focus();
      }
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not switch account", "error");
    }
  };

  const storePassword = async (name: string) => {
    setAction("store-password");
    setMethod("password");
    setUsername(name);
    setPassword("");
    say(
      `Enter the Steam password for ${name}. It is encrypted with Windows and kept on this PC only, so downloads can sign in on their own.`,
      "warn",
    );
    passwordRef.current?.focus();
  };

  const removeAccount = async (name: string) => {
    if (!window.confirm(`Remove ${name} from WallForge?`)) return;
    try {
      const result = await api.deleteAccount(name);
      if (!result.success) throw new Error(result.message || "Could not remove that account");
      setAccounts(result.accounts ?? []);
      setActive(result.active ?? null);
      say(`${name} removed.`, "success");
    } catch (err) {
      say(
        err instanceof Error ? err.message : "Could not remove that account",
        "error",
      );
    }
  };

  const saveStoredPassword = async () => {
    if (!username || !password) {
      say("Enter the Steam username and password to save.", "error");
      return;
    }
    try {
      const result = await api.saveAccountPassword(username, password);
      if (!result.success) throw new Error(result.message || "Could not save that password");
      setAccounts(result.accounts ?? accounts);
      setPassword("");
      setAction("signin");
      say(
        `Password saved for ${username}. Downloads will now sign in on their own with it.`,
        "success",
      );
    } catch (err) {
      say(
        err instanceof Error ? err.message : "Could not save that password",
        "error",
      );
    }
  };

  const startLogin = async () => {
    if (busy) return;
    if (action === "store-password") {
      await saveStoredPassword();
      return;
    }
    if (method === "password" && (!username || !password)) {
      say("Enter your Steam username and password.", "error");
      return;
    }
    setBusy(true);
    say("");
    try {
      await api.startLogin(method, username, password);
      setPassword("");
      onRefresh();
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not sign in", "error");
    } finally {
      setBusy(false);
    }
  };

  const submitCode = async () => {
    if (!code) {
      say("Enter the Steam Guard code.", "error");
      return;
    }
    try {
      await api.submitLoginCode(code);
      setCode("");
      onRefresh();
    } catch (err) {
      say(err instanceof Error ? err.message : "Could not submit the code", "error");
    }
  };

  const cancel = async () => {
    if (state === "starting" || state === "confirm" || state === "code") {
      try {
        await api.cancelLogin();
      } catch (err) {
        console.error("cancel_login failed", err);
      }
    }
    onClose();
  };

  const busyLogin = state === "starting" || state === "confirm" || state === "code";

  return (
    <div
      className="dialog-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) cancel();
      }}
    >
      <div
        className="dialog login-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="login-dialog-title"
        ref={dialogRef}
      >
        <div className="dialog-inner">
          <div className="dialog-head">
            <div>
              <p className="eyebrow">Steam account</p>
              <h2 id="login-dialog-title">Sign in to Steam</h2>
            </div>
            <button
              type="button"
              className="icon-btn"
              aria-label="Close"
              onClick={cancel}
            >
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

          <p className="hint">
            Downloads run through your own Steam account. A sign-in password is used once and
            never written to disk. A password you save for an account is encrypted with
            Windows and kept on this PC.
          </p>

          <div className="segmented" role="radiogroup" aria-label="Sign-in method">
            <button
              type="button"
              className={`segment${method === "password" && action === "signin" ? " active" : ""}`}
              data-login-method="password"
              role="radio"
              aria-checked={method === "password" && action === "signin"}
              disabled={busyLogin}
              onClick={() => {
                setMethod("password");
                setAction("signin");
                say("");
              }}
            >
              Username &amp; password
            </button>
            <button
              type="button"
              className={`segment${method === "qr" ? " active" : ""}`}
              data-login-method="qr"
              role="radio"
              aria-checked={method === "qr"}
              disabled={busyLogin}
              onClick={() => {
                setMethod("qr");
                setAction("signin");
                say("");
              }}
            >
              QR code
            </button>
          </div>

          {method === "password" ? (
            <div className="dialog-section">
              <label className="field-label" htmlFor="login-username">
                Steam username
              </label>
              <input
                id="login-username"
                className="input"
                type="text"
                autoComplete="username"
                spellCheck={false}
                placeholder="Your Steam account name"
                value={username}
                onChange={(event) => setUsername(event.target.value)}
              />
              <label className="field-label" htmlFor="login-password">
                Password
              </label>
              <input
                id="login-password"
                className="input"
                type="password"
                autoComplete="current-password"
                placeholder={
                  action === "store-password"
                    ? "Encrypted and kept on this PC"
                    : "Used once, never saved"
                }
                value={password}
                ref={passwordRef}
                onChange={(event) => setPassword(event.target.value)}
              />
            </div>
          ) : null}

          {method === "qr" ? (
            <div className="dialog-section">
              <div className="qr-box">
                {status?.has_qr && qrSrc ? (
                  <img className="qr-image" alt="Steam sign-in QR code" src={qrSrc} />
                ) : (
                  <p className="qr-placeholder">
                    {state === "starting"
                      ? "Connecting to Steam."
                      : "Press Show QR code, then scan it with the Steam mobile app."}
                  </p>
                )}
              </div>
              <p className="hint">
                Open the Steam mobile app, scan the code, then approve the sign-in.
              </p>
            </div>
          ) : null}

          {state === "code" ? (
            <div className="dialog-section">
              <label className="field-label" htmlFor="login-code">
                Steam Guard code
              </label>
              <div className="path-row">
                <input
                  id="login-code"
                  className="input"
                  type="text"
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  spellCheck={false}
                  placeholder="Code from the app or email"
                  value={code}
                  onChange={(event) => setCode(event.target.value)}
                />
                <button type="button" className="secondary" onClick={submitCode}>
                  Submit
                </button>
              </div>
            </div>
          ) : null}

          <p className={`dialog-message${tone ? ` ${tone}` : ""}`} role="status">
            {message}
          </p>

          <div className="dialog-section">
            <p className="field-label">Saved accounts</p>
            <div className="account-list">
              {accounts.length === 0 ? (
                <p className="account-empty">
                  No saved accounts yet. Sign in once and the username is remembered.
                </p>
              ) : null}
              {accounts.map((entry) => {
                const ready = entry.signed_in || entry.has_password;
                return (
                  <div
                    className={`account-row${entry.username === active ? " active" : ""}`}
                    key={entry.username}
                  >
                    <span className="account-name" title={entry.username}>
                      {entry.username}
                    </span>
                    {entry.username === active ? (
                      <span className="account-tag">In use</span>
                    ) : null}
                    <span
                      className={`account-tag${ready ? "" : " needs-signin"}`}
                      title={
                        entry.has_password
                          ? "A password is saved on this PC, so downloads sign in on their own."
                          : entry.signed_in
                            ? "This account is signed in on this PC."
                            : "Sign in once, or save the password, before downloading."
                      }
                    >
                      {ready ? "Ready" : "Needs sign-in"}
                    </span>
                    {entry.built_in ? (
                      <span
                        className="account-tag built-in"
                        title="This account ships with WallForge, so it cannot be changed or removed."
                      >
                        Built in
                      </span>
                    ) : null}
                    {entry.username !== active ? (
                      <button
                        type="button"
                        className="ghost"
                        onClick={() => useAccount(entry.username, ready)}
                      >
                        Use
                      </button>
                    ) : null}
                    {entry.built_in ? null : (
                      <>
                        <button
                          type="button"
                          className="ghost"
                          onClick={() => storePassword(entry.username)}
                        >
                          {entry.has_password ? "Change password" : "Password"}
                        </button>
                        <button
                          type="button"
                          className="ghost"
                          onClick={() => removeAccount(entry.username)}
                        >
                          Remove
                        </button>
                      </>
                    )}
                  </div>
                );
              })}
            </div>
          </div>

          <div className="dialog-actions">
            <button type="button" className="ghost" onClick={cancel}>
              {busyLogin ? "Cancel sign-in" : "Close"}
            </button>
            <button
              type="button"
              className="primary"
              onClick={startLogin}
              disabled={busyLogin}
            >
              {action === "store-password"
                ? "Save password"
                : method === "qr"
                  ? "Show QR code"
                  : "Sign in"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

