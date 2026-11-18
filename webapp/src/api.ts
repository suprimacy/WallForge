import type {
  AccountsPayload,
  ActionResult,
  DownloadJob,
  DownloadStatus,
  ExperimentalStatus,
  FilterGroup,
  InstallInfo,
  ItemDetails,
  LoginStatus,
  SearchResult,
  WallpaperItem,
} from "./types";

// The interface talks to one of two backends:
//   - the Rust core, through Tauri commands (window.__TAURI_INTERNALS__)
//   - the Python backend, through the local HTTP API
// Both expose the same calls, so the interface never changes.

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

function tauriInvoke(): Invoke | null {
  const internals = (window as unknown as {
    __TAURI_INTERNALS__?: { invoke?: Invoke };
  }).__TAURI_INTERNALS__;
  return internals && typeof internals.invoke === "function" ? internals.invoke : null;
}

export const isTauri = () => tauriInvoke() !== null;

const API_BASE = "/api";

async function httpRequest<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
  if (!response.ok) {
    let message = `Request failed (${response.status})`;
    try {
      const body = await response.json();
      if (body && typeof body.message === "string") message = body.message;
    } catch {
      /* the body was not json */
    }
    throw new Error(message);
  }
  return (await response.json()) as T;
}

function post<T>(path: string, body?: unknown): Promise<T> {
  return httpRequest<T>(path, { method: "POST", body: JSON.stringify(body ?? {}) });
}

/** Run a call against whichever backend is running. */
async function call<T>(
  command: string,
  args: Record<string, unknown>,
  fallback: () => Promise<T>,
): Promise<T> {
  const invoke = tauriInvoke();
  if (!invoke) return fallback();
  try {
    return await invoke<T>(command, args);
  } catch (err) {
    // A command that returns Err carries the message we want to show.
    if (typeof err === "string") throw new Error(err);
    if (err instanceof Error) throw err;
    throw new Error(String(err));
  }
}

export const api = {
  info: () =>
    call<InstallInfo>("get_info", {}, () => httpRequest<InstallInfo>("/info")),

  accounts: () =>
    call<AccountsPayload>("get_accounts", {}, () => httpRequest<AccountsPayload>("/accounts")),

  selectAccount: (username: string) =>
    call<ActionResult>(
      "select_account",
      { username },
      () => post<ActionResult>("/accounts/active", { username }),
    ),

  saveAccountPassword: (username: string, password: string) =>
    call<ActionResult>(
      "save_account_password",
      { username, password },
      () => post<ActionResult>("/accounts/password", { username, password }),
    ),

  deleteAccount: (username: string) =>
    call<ActionResult>(
      "delete_account",
      { username },
      () =>
        httpRequest<ActionResult>(`/accounts/${encodeURIComponent(username)}`, {
          method: "DELETE",
        }),
    ),

  loginStatus: () =>
    call<LoginStatus>("login_status", {}, () => httpRequest<LoginStatus>("/login/status")),

  startLogin: (method: "password" | "qr", username: string, password: string) =>
    call<LoginStatus>(
      "start_login",
      { method, username, password },
      () => post<LoginStatus>("/login", { method, username, password }),
    ),

  submitLoginCode: (code: string) =>
    call<LoginStatus>(
      "submit_login_code",
      { code },
      () => post<LoginStatus>("/login/code", { code }),
    ),

  cancelLogin: () =>
    call<ActionResult>("cancel_login", {}, () => post<ActionResult>("/login/cancel")),

  filters: () =>
    call<{ groups: FilterGroup[] }>(
      "get_filters",
      {},
      () => httpRequest<{ groups: FilterGroup[] }>("/filters"),
    ),

  search: (
    searchtext: string,
    page: number,
    sortmethod: string,
    requiredTags: string[],
  ) =>
    call<SearchResult>(
      "search_workshop",
      { searchtext, page, sortmethod, timeperiod: "-1", requiredTags },
      () => {
        const params = new URLSearchParams({
          searchtext,
          page: String(page),
          sortmethod,
          timeperiod: "-1",
        });
        requiredTags.forEach((tag) => params.append("requiredtags[]", tag));
        return httpRequest<SearchResult>(`/search?${params.toString()}`);
      },
    ),

  item: (id: string) =>
    call<WallpaperItem>(
      "get_item",
      { workshopId: id },
      () => httpRequest<WallpaperItem>(`/item/${encodeURIComponent(id)}`),
    ),

  itemDetails: (id: string) =>
    call<ItemDetails>(
      "get_item_details",
      { workshopId: id },
      () => httpRequest<ItemDetails>(`/item/${encodeURIComponent(id)}/details`),
    ),

  downloads: (sort = "newest") =>
    call<{ items: WallpaperItem[] }>(
      "list_downloads",
      { sort },
      () => httpRequest<{ items: WallpaperItem[] }>("/downloads"),
    ),

  download: (workshopId: string, meta?: WallpaperItem) =>
    call<{ started: boolean; success: boolean; job_id: string; message?: string }>(
      "download",
      { workshopId, title: meta?.title ?? "", meta: meta ?? null },
      () =>
        post<{ started: boolean; success: boolean; job_id: string; message?: string }>(
          "/download",
          { workshop_id: workshopId },
        ),
    ),

  downloadStatus: (jobId: string) =>
    call<DownloadStatus>(
      "download_status",
      { jobId },
      () => httpRequest<DownloadStatus>(`/download-status/${encodeURIComponent(jobId)}`),
    ),

  downloadQueue: () =>
    call<{ jobs: DownloadJob[] }>(
      "download_queue",
      {},
      () => httpRequest<{ jobs: DownloadJob[] }>("/download-queue"),
    ),

  downloadPause: (jobId: string) =>
    call<{ success: boolean }>(
      "download_pause",
      { jobId },
      () => post<{ success: boolean }>("/download-pause", { job_id: jobId }),
    ),

  downloadResume: (jobId: string) =>
    call<{ success: boolean }>(
      "download_resume",
      { jobId },
      () => post<{ success: boolean }>("/download-resume", { job_id: jobId }),
    ),

  downloadCancel: (jobId: string) =>
    call<{ success: boolean }>(
      "download_cancel",
      { jobId },
      () => post<{ success: boolean }>("/download-cancel", { job_id: jobId }),
    ),

  downloadForget: (jobId: string) =>
    call<{ success: boolean }>(
      "download_forget",
      { jobId },
      () => post<{ success: boolean }>("/download-forget", { job_id: jobId }),
    ),

  downloadClear: () =>
    call<{ success: boolean; cleared: number }>(
      "download_clear",
      {},
      () => post<{ success: boolean; cleared: number }>("/download-clear", {}),
    ),

  deleteDownload: (workshopId: string) =>
    call<ActionResult>(
      "delete_download",
      { workshopId },
      () =>
        httpRequest<ActionResult>(`/download/${encodeURIComponent(workshopId)}`, {
          method: "DELETE",
        }),
    ),

  openFolder: (workshopId: string) =>
    call<ActionResult>(
      "open_folder",
      { workshopId },
      () => post<ActionResult>("/open-folder", { workshop_id: workshopId }),
    ),

  openUrl: (url: string) =>
    call<ActionResult>(
      "open_url",
      { url },
      () => post<ActionResult>("/open-url", { url }),
    ),

  /* ---- experimental ManifestHub manifests ---- */

  experimentalStatus: () =>
    call<ExperimentalStatus>(
      "experimental_status",
      {},
      () => httpRequest<ExperimentalStatus>("/experimental"),
    ),

  experimentalSetEnabled: (enabled: boolean) =>
    call<ActionResult>(
      "experimental_set_enabled",
      { enabled },
      () => post<ActionResult>("/experimental", { enabled }),
    ),

  authorWallpapers: (authorUrl: string) =>
    call<SearchResult>(
      "author_wallpapers",
      { authorUrl },
      () => post<SearchResult>("/author-wallpapers", { author_url: authorUrl }),
    ),

  browseInstallDir: () =>
    call<{ success: boolean; cancelled: boolean; path: string | null; message?: string }>(
      "browse_install_dir",
      {},
      () => post<{ success: boolean; cancelled: boolean; path: string | null; message?: string }>("/install-dir/browse"),
    ),

  setInstallDir: (path: string) =>
    call<ActionResult>(
      "set_install_dir",
      { path },
      () => post<ActionResult>("/install-dir", { path }),
    ),

  resetInstallDir: () =>
    call<ActionResult>(
      "reset_install_dir",
      {},
      () => httpRequest<ActionResult>("/install-dir", { method: "DELETE" }),
    ),

  /** The QR code as something the interface can put in an <img>. */
  qrImage: async (version: number): Promise<string> => {
    const invoke = tauriInvoke();
    if (invoke) {
      const result = await invoke<{ success: boolean; data_url: string }>("qr_svg");
      return result.success ? result.data_url : "";
    }
    return `/api/login/qr.svg?v=${version}`;
  },
};
