export interface WallpaperItem {
  id: string;
  title: string;
  author: string;
  img: string;
  link: string;
  rating: string;
  path?: string;
  pathShort?: string;
  added?: number;
}

export interface SearchResult {
  items: WallpaperItem[];
  page: number;
  total_pages: number;
  total_count: number;
}

export interface FilterGroup {
  name: string;
  type: string;
  tags: string[];
}

export interface DetailFact {
  group: string;
  name: string;
}

export interface ItemDetails {
  id: string;
  title: string;
  author: string;
  author_url: string;
  img: string;
  images: string[];
  video_id: string;
  link: string;
  rating: string;
  rating_count: string;
  file_size: string;
  posted: string;
  updated: string;
  tags: DetailFact[];
  description: string;
  /** True when Steam served an unreadable item page, so only the basics came from the Web API. */
  partial?: boolean;
}

export interface AccountEntry {
  username: string;
  signed_in: boolean;
  has_password: boolean;
  built_in: boolean;
}

export interface AccountsPayload {
  accounts: AccountEntry[];
  active: string | null;
}

export interface LoginStatus {
  status: "idle" | "starting" | "confirm" | "code" | "success" | "error";
  message: string;
  username: string | null;
  has_qr: boolean;
  qr_version: number;
  accounts: AccountEntry[];
  active: string | null;
}

export interface DownloadStatus {
  status: string;
  progress: number;
  message: string;
  path?: string | null;
}

export type DownloadState =
  | "queued"
  | "downloading"
  | "paused"
  | "done"
  | "failed"
  | "cancelled";

export interface DownloadJob {
  id: string;
  title: string;
  img?: string | null;
  state: DownloadState;
  progress: number;
  message: string;
  path?: string | null;
  /** "TOKEN_REJECTED" on a dead ManifestHub key, so the UI can ask for a new one. */
  error_code?: string | null;
}

export type TokenState = "missing" | "valid" | "expired";

/** A ManifestHub key and its derived 24h state. */
export interface TokenStatus {
  token: string;
  savedAt: number;
  expiresAt: number;
  /** Backend clock at measurement time; anchor the countdown here. */
  measuredAt: number;
  remainingSecs: number;
  state: TokenState;
}

export interface SaveResult {
  status: TokenStatus;
  warning: string | null;
}

export interface ExperimentalStatus {
  enabled: boolean;
  tokenStatus: TokenStatus;
}

export interface InstallInfo {
  version?: string;
  install_dir?: string | null;
  auto_install_dir?: string | null;
  auto_candidates?: string[];
  install_source?: string;
  depot_exists?: boolean;
  download_dir?: string | null;
}

export interface ActionResult {
  success: boolean;
  message?: string;
  accounts?: AccountEntry[];
  active?: string | null;
}
