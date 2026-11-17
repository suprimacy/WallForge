import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { useStoredValue } from "./hooks";
import type {
  DownloadJob,
  ExperimentalStatus,
  FilterGroup,
  ItemDetails,
  WallpaperItem,
} from "./types";
import { WallpaperCard } from "./components/WallpaperCard";
import { DetailsDialog } from "./components/DetailsDialog";
import { LoginDialog } from "./components/LoginDialog";
import { InstallDialog } from "./components/InstallDialog";
import { ExperimentalDialog } from "./components/ExperimentalDialog";
import { DownloadQueue } from "./components/DownloadQueue";
import { FilterDrawer } from "./components/FilterDrawer";
import { Pagination } from "./components/Pagination";
import { Notice } from "./components/Notice";

const FILTER_STORAGE_KEY = "wallforge.filters.v1";
const DOWNLOAD_SORT_STORAGE_KEY = "wallforge.downloadSort.v1";
const QUEUE_STORAGE_KEY = "wallforge.queueOpen.v1";
const DOWNLOAD_SORT_MODES = ["newest", "oldest"];

const SORTS = [
  ["top_rated", "Top Rated"],
  ["trend_year", "Most Popular (Year)"],
  ["trend_month", "Most Popular (Month)"],
  ["trend_week", "Most Popular (Week)"],
  ["trend_today", "Most Popular (Today)"],
  ["most_recent", "Most Recent"],
  ["most_up_votes", "Most Up Votes"],
  ["subscriptions", "Most Subscriptions"],
];

function formatAddedDate(stamp?: number): string {
  if (!stamp) return "";
  return new Date(stamp * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export default function App() {
  const [tab, setTab] = useState<"top" | "downloads">("top");
  const [search, setSearch] = useState("");
  const [authorFilter, setAuthorFilter] = useState<{ name: string; url: string } | null>(null);
  const [sort, setSort] = useState("top_rated");
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(0);
  const [totalCount, setTotalCount] = useState(0);
  const [items, setItems] = useState<WallpaperItem[]>([]);
  const [status, setStatus] = useState("Ready");
  const [loading, setLoading] = useState(false);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [expandedFilters, setExpandedFilters] = useState<string[]>([]);
  const [filterGroups, setFilterGroups] = useState<FilterGroup[]>([]);
  const [selectedTags, setSelectedTags] = useStoredValue<string[]>(
    FILTER_STORAGE_KEY,
    [],
  );
  const [downloadSort, setDownloadSort] = useStoredValue<string>(
    DOWNLOAD_SORT_STORAGE_KEY,
    "newest",
  );

  const [downloads, setDownloads] = useState<WallpaperItem[]>([]);
  const [downloadStatus, setDownloadStatus] = useState("No downloads yet");
  const [queue, setQueue] = useState<DownloadJob[]>([]);
  const [queueOpen, setQueueOpen] = useStoredValue<boolean>(QUEUE_STORAGE_KEY, true);
  const [notice, setNotice] = useState<string | null>(null);

  const [detailItem, setDetailItem] = useState<WallpaperItem | null>(null);
  const [details, setDetails] = useState<ItemDetails | null>(null);
  const [detailsError, setDetailsError] = useState<string | null>(null);
  const [detailsLoading, setDetailsLoading] = useState(false);

  const [loginOpen, setLoginOpen] = useState(false);
  const [loginStatus, setLoginStatus] = useState<Awaited<
    ReturnType<typeof api.loginStatus>
  > | null>(null);
  const [installOpen, setInstallOpen] = useState(false);
  const [experimentalOpen, setExperimentalOpen] = useState(false);
  const [experimental, setExperimental] = useState<ExperimentalStatus | null>(null);
  const tokenRejectSeen = useRef(false);
  const [installInfo, setInstallInfo] = useState<Awaited<
    ReturnType<typeof api.info>
  > | null>(null);
  const [steamLabel, setSteamLabel] = useState("Sign in to Steam");

  const searchSeq = useRef(0);
  const loginPoll = useRef<number | null>(null);

  /* ---------------- data loading ---------------- */

  const loadDownloads = useCallback(async () => {
    try {
      const data = await api.downloads();
      const list = data.items ?? [];
      setDownloads(
        [...list].sort((a, b) =>
          downloadSort === "oldest"
            ? (a.added ?? 0) - (b.added ?? 0)
            : (b.added ?? 0) - (a.added ?? 0),
        ),
      );
      setDownloadStatus(`${list.length} downloaded`);
    } catch (err) {
      console.error("list_downloads failed", err);
      setDownloadStatus("Failed to load downloads");
    }
  }, [downloadSort]);

  const loadWorkshop = useCallback(async () => {
    const term = search.trim();
    const seq = ++searchSeq.current;
    setLoading(true);
    try {
      // The author view: items published by one creator, from their files page.
      if (authorFilter) {
        const data = await api.authorWallpapers(authorFilter.url);
        if (seq !== searchSeq.current) return;
        const items = (data.items ?? []).map((item) => ({
          ...item,
          author: item.author || authorFilter.name,
        }));
        setItems(items);
        setTotalPages(1);
        setTotalCount(items.length);
        setPage(1);
        setStatus(
          items.length
            ? `${items.length} wallpaper${items.length === 1 ? "" : "s"} by ${authorFilter.name}`
            : `No wallpapers by ${authorFilter.name}`,
        );
        void loadDownloads();
        return;
      }
      if (term && /^\d{6,}$/.test(term)) {
        const item = await api.item(term);
        if (seq !== searchSeq.current) return;
        setItems([item]);
        setTotalPages(0);
        setTotalCount(0);
        setStatus(`Showing item ${term}`);
        return;
      }
      const data = await api.search(term, page, sort, selectedTags);
      if (seq !== searchSeq.current) return;
      setItems(data.items ?? []);
      if (typeof data.page === "number" && data.page > 0) setPage(data.page);
      setTotalPages(data.total_pages ?? 0);
      setTotalCount(data.total_count ?? 0);
      setStatus(
        data.total_count
          ? `Showing ${data.items?.length ?? 0} of ${data.total_count.toLocaleString()} wallpapers`
          : "No wallpapers found",
      );
      // Keep the library in step, so a card that is already downloaded
      // offers Open folder and Remove.
      void loadDownloads();
    } catch (err) {
      if (seq !== searchSeq.current) return;
      setStatus("Search failed");
      console.error("search failed", err);
    } finally {
      if (seq === searchSeq.current) setLoading(false);
    }
  }, [search, page, sort, selectedTags, loadDownloads, authorFilter]);

  const loadAccountInfo = useCallback(async () => {
    try {
      const payload = await api.accounts();
      setSteamLabel(payload.active ?? "Sign in to Steam");
    } catch {
      setSteamLabel("Sign in to Steam");
    }
  }, []);

  useEffect(() => {
    void loadWorkshop();
  }, [loadWorkshop]);

  useEffect(() => {
    void loadDownloads();
  }, [loadDownloads]);

  useEffect(() => {
    if (tab === "downloads") void loadDownloads();
  }, [tab, loadDownloads]);

  // The queue is polled while there is something to show.
  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const payload = await api.downloadQueue();
        const jobs = payload.jobs ?? [];
        if (!cancelled) setQueue(jobs);
        const rejected = jobs.some((job) => job.error_code === "TOKEN_REJECTED");
        if (!cancelled && rejected && !tokenRejectSeen.current) {
          tokenRejectSeen.current = true;
          setNotice("ManifestHub rejected the key. Paste a new one to keep downloading without Steam.");
          void api.experimentalStatus().then(setExperimental).catch(() => undefined);
          setExperimentalOpen(true);
        }
        if (!rejected) tokenRejectSeen.current = false;
      } catch {
        /* the queue is not available */
      }
    };
    void tick();
    const timer = window.setInterval(tick, 900);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

  // Lock the page behind any open dialog or drawer.
  useEffect(() => {
    const modalOpen = Boolean(
      detailItem || loginOpen || installOpen || experimentalOpen || filtersOpen,
    );
    document.body.classList.toggle("modal-open", modalOpen);
    return () => document.body.classList.remove("modal-open");
  }, [detailItem, loginOpen, installOpen, experimentalOpen, filtersOpen]);

  useEffect(() => {
    void loadAccountInfo();
    void api.info().then(setInstallInfo).catch(() => undefined);
    void api.experimentalStatus().then(setExperimental).catch(() => undefined);
  }, [loadAccountInfo]);

  /* ---------------- filters ---------------- */

  const loadFilters = useCallback(async () => {
    try {
      const data = await api.filters();
      setFilterGroups(data.groups ?? []);
    } catch (err) {
      console.error("get_filters failed", err);
      setFilterGroups([]);
    }
  }, []);

  useEffect(() => {
    if (filtersOpen) void loadFilters();
  }, [filtersOpen, loadFilters]);

  const toggleTag = (tag: string, single: boolean) => {
    setSelectedTags((current) => {
      const next = new Set(current);
      if (next.has(tag)) {
        next.delete(tag);
      } else {
        if (single) {
          filterGroups.forEach((group) => {
            if (group.type !== "checkbox" && group.tags.includes(tag)) {
              group.tags.forEach((other) => next.delete(other));
            }
          });
        }
        next.add(tag);
      }
      return [...next];
    });
    setPage(1);
  };

  const filterCount = selectedTags.length;

  /* ---------------- downloads ---------------- */

  // Clear every queued/finished entry from the list, keeping active downloads.
  const clearQueue = useCallback(async () => {
    try {
      await api.downloadClear();
      setQueue((current) =>
        current.filter((job) => job.state === "downloading" || job.state === "paused"),
      );
    } catch (err) {
      console.error("download_clear failed", err);
      setNotice("The download list could not be cleared.");
    }
  }, []);

  // Open a wallpaper's folder, and say so when Windows refuses.
  const openFolder = useCallback(async (workshopId: string) => {
    try {
      const result = await api.openFolder(workshopId);
      if (!result.success) {
        setNotice(result.message || "Windows would not open that folder.");
      }
    } catch (err) {
      console.error("open folder failed", err);
      setNotice(err instanceof Error ? err.message : String(err));
    }
  }, []);

  // Remove a wallpaper from the library.
  const removeDownload = useCallback(
    async (workshopId: string) => {
      try {
        const result = await api.deleteDownload(workshopId);
        if (!result.success) {
          setNotice(result.message || "That wallpaper could not be removed.");
          return;
        }
        await loadDownloads();
      } catch (err) {
        console.error("remove failed", err);
        setNotice(err instanceof Error ? err.message : String(err));
      }
    },
    [loadDownloads],
  );

  const runDownload = useCallback(
    async (item: WallpaperItem) => {
      try {
        const started = await api.download(item.id, item);
        if (!started.started && !started.success) {
          throw new Error(started.message || "Failed to start download");
        }
        setDownloadStatus("Queued");
        setDetailItem(null);
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        setDownloadStatus(`Failed: ${message}`);
        if (/sign in to steam first/i.test(message) || /session expired/i.test(message)) {
          setLoginOpen(true);
        }
        console.error("download failed", err);
      }
    },
    [],
  );

  /* ---------------- details ---------------- */

  // Closed via a stable identity so the dialog does not re-grab focus on every render.
  const closeDetails = useCallback(() => setDetailItem(null), []);

  /* ---------------- author view ---------------- */

  // Show only the wallpapers published by one author, from their Workshop files page.
  const handleOpenAuthor = useCallback((name: string, url: string) => {
    setAuthorFilter({ name, url });
    setTab("top");
    setPage(1);
    setDetailItem(null);
  }, []);

  const clearAuthorFilter = useCallback(() => {
    setAuthorFilter(null);
    setPage(1);
  }, []);

  const loadDetails = useCallback(async (item: WallpaperItem) => {
    setDetailsLoading(true);
    setDetailsError(null);
    try {
      const payload = await api.itemDetails(item.id);
      setDetails(payload);
    } catch (err) {
      setDetailsError(err instanceof Error ? err.message : String(err));
    } finally {
      setDetailsLoading(false);
    }
  }, []);

  const openDetails = useCallback(
    (item: WallpaperItem) => {
      setDetailItem(item);
      setDetails(null);
      void loadDetails(item);
    },
    [loadDetails],
  );

  /* ---------------- steam login ---------------- */

  const refreshLoginStatus = useCallback(async () => {
    try {
      const status = await api.loginStatus();
      setLoginStatus(status);
      setSteamLabel(status.active ?? "Sign in to Steam");
      if (status.status === "success") {
        window.setTimeout(() => setLoginOpen(false), 900);
        void loadDownloads();
      }
    } catch (err) {
      console.error("login_status failed", err);
    }
  }, [loadDownloads]);

  useEffect(() => {
    if (!loginOpen) {
      if (loginPoll.current) window.clearInterval(loginPoll.current);
      loginPoll.current = null;
      return;
    }
    void refreshLoginStatus();
    loginPoll.current = window.setInterval(() => {
      void refreshLoginStatus();
    }, 1200);
    return () => {
      if (loginPoll.current) window.clearInterval(loginPoll.current);
      loginPoll.current = null;
    };
  }, [loginOpen, refreshLoginStatus]);

  const downloadedIds = useMemo(
    () => new Set(downloads.map((item) => item.id)),
    [downloads],
  );

  const resultsLabel = totalCount
    ? `${items.length} of ${totalCount.toLocaleString()} wallpapers`
    : "";

  return (
    <>
      <div className="aurora" aria-hidden="true" />

      <header className="topbar">
        <div className="brand">
          <div className="brand-mark" aria-hidden="true">
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.7"
              strokeLinecap="round"
              strokeLinejoin="round"
              focusable="false"
            >
              <rect x="2.5" y="3.5" width="19" height="13.5" rx="2.6" />
              <path d="M8.5 21h7M12 17v4" />
              <path d="M2.6 13.2 7 8.8a1.9 1.9 0 0 1 2.7 0l4.4 4.4" />
              <circle cx="16.6" cy="8.1" r="1.5" />
            </svg>
          </div>
          <div className="brand-text">
            <h1>WallForge</h1>
          </div>
        </div>

        <div className="search-bar">
          <div className="search-field">
            <input
              id="search"
              type="text"
              placeholder="Search wallpapers or paste a workshop ID..."
              autoComplete="off"
              spellCheck={false}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  setTab("top");
                  clearAuthorFilter();
                  setPage(1);
                  void loadWorkshop();
                }
              }}
            />
          </div>
          <button
            id="refresh-btn"
            className="primary search-btn"
            type="button"
            aria-label="Search"
            title="Search"
            onClick={() => {
              setTab("top");
              clearAuthorFilter();
              setPage(1);
              void loadWorkshop();
            }}
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              aria-hidden="true"
              focusable="false"
            >
              <circle cx="11" cy="11" r="7" />
              <path d="m20 20-3.6-3.6" />
            </svg>
          </button>
        </div>

        <div className="topbar-actions">
          <button
            id="steam-info"
            type="button"
            className="pill muted steam-pill warn"
            title="Sign in to Steam to download wallpapers"
            onClick={() => setLoginOpen(true)}
          >
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
              <circle cx="12" cy="8" r="3.4" />
              <path d="M5.5 20a6.5 6.5 0 0 1 13 0" />
            </svg>
            <span>{steamLabel}</span>
          </button>
          <button
            id="experimental-info"
            type="button"
            className={`pill muted experimental-pill${experimental?.enabled ? " active" : ""}`}
            title="ManifestHub key and experimental downloads"
            onClick={() => {
              void api.experimentalStatus().then(setExperimental).catch(() => undefined);
              setExperimentalOpen(true);
            }}
          >
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
              <path d="M9 3h6M10 3v6.2L5.6 17a2 2 0 0 0 1.7 3h9.4a2 2 0 0 0 1.7-3L14 9.2V3" />
              <path d="M7.8 14h8.4" />
            </svg>
            <span>Experimental</span>
          </button>
          <button
            id="install-info"
            type="button"
            className="pill muted install-pill"
            title="Change the Wallpaper Engine install folder"
            onClick={() => {
              void api.info().then(setInstallInfo).catch(() => undefined);
              setInstallOpen(true);
            }}
          >
            {installInfo?.install_dir ?? installInfo?.auto_install_dir ?? "Detecting install..."}
          </button>
        </div>
      </header>

      <div className="app-body">
      <main>
        <div className="view-bar">
          <div className="segmented" role="tablist" aria-label="Views">
            <button
              className={`segment${tab === "top" ? " active" : ""}`}
              data-tab="top"
              role="tab"
              aria-selected={tab === "top"}
              onClick={() => setTab("top")}
            >
              Browse
            </button>
            <button
              className={`segment${tab === "downloads" ? " active" : ""}`}
              data-tab="downloads"
              role="tab"
              aria-selected={tab === "downloads"}
              onClick={() => setTab("downloads")}
            >
              Library
            </button>
          </div>

          <div className="view-bar-actions">
            <span id="status" className="view-bar-meta" role="status" aria-live="polite">
              {tab === "top"
                ? loading
                  ? "Loading..."
                  : resultsLabel || status
                : downloadStatus}
            </span>
            {tab === "top" ? (
              <button
                id="filter-toggle"
                className="secondary"
                aria-expanded={filtersOpen}
                aria-controls="filters"
                onClick={() => setFiltersOpen(true)}
              >
                Filters
                {filterCount ? <span className="badge">{filterCount}</span> : null}
              </button>
            ) : null}
          </div>
        </div>

        <section id="top" className={`panel${tab === "top" ? " active" : ""}`}>
          <div className="sort-row">
            <div className="sort-field">
              <label className="sort-label" htmlFor="sort-method">
                Sort
              </label>
              <select
                id="sort-method"
                className="select"
                value={sort}
                onChange={(event) => {
                  setSort(event.target.value);
                  setPage(1);
                }}
              >
                {SORTS.map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {filterCount || authorFilter ? (
            <div id="active-filters" className="active-filters" aria-live="polite">
              <span className="active-filters-label">Filters</span>
              {authorFilter ? (
                <span className="filter-chip">
                  by {authorFilter.name}
                  <button
                    type="button"
                    aria-label={`Remove the ${authorFilter.name} filter`}
                    onClick={clearAuthorFilter}
                  >
                    ×
                  </button>
                </span>
              ) : null}
              {selectedTags.map((tag) => (
                <span className="filter-chip" key={tag}>
                  {tag}
                  <button
                    type="button"
                    aria-label={`Remove ${tag}`}
                    onClick={() => toggleTag(tag, false)}
                  >
                    ×
                  </button>
                </span>
              ))}
            </div>
          ) : null}

          <Pagination
            className="top-pagination"
            page={page}
            totalPages={totalPages}
            onPage={setPage}
          />

          <div id="grid" className="grid">
            {items.map((item) => (
              <WallpaperCard
                key={item.id}
                item={item}
                downloaded={downloadedIds.has(item.id)}
                busy={false}
                busyLabel="Downloading..."

                onOpen={() => openDetails(item)}
                onDownload={() => void runDownload(item)}
                onOpenFolder={() => void openFolder(item.id)}
                onRemove={() => void removeDownload(item.id)}
              />
            ))}
          </div>

          <Pagination
            className="bottom-pagination"
            page={page}
            totalPages={totalPages}
            onPage={setPage}
          />
        </section>

        <section id="downloads" className={`panel${tab === "downloads" ? " active" : ""}`}>
          <div className="view-bar-actions" style={{ marginBottom: 16 }}>
            <div className="sort-field">
              <label className="sort-label" htmlFor="download-sort">
                Sort
              </label>
              <select
                id="download-sort"
                className="select"
                value={DOWNLOAD_SORT_MODES.includes(downloadSort) ? downloadSort : "newest"}
                onChange={(event) => setDownloadSort(event.target.value)}
              >
                <option value="newest">Newest first</option>
                <option value="oldest">Oldest first</option>
              </select>
            </div>
            <button
              id="refresh-downloads"
              className="secondary"
              onClick={() => void loadDownloads()}
            >
              Reload library
            </button>
          </div>
          <div id="download-grid" className="grid">
            {downloads.map((item) => (
              <WallpaperCard
                key={item.id}
                item={item}
                downloaded
                busy={false}
                busyLabel=""
                onOpen={() => openDetails(item)}
                onDownload={() => undefined}
                onOpenFolder={() => void openFolder(item.id)}
                onRemove={() => void removeDownload(item.id)}
              >
                <p className="added">{formatAddedDate(item.added)}</p>
                <p className="path">{item.pathShort || item.path}</p>
              </WallpaperCard>
            ))}
          </div>
        </section>
      </main>

      <DownloadQueue
        jobs={queue}
        open={queueOpen}
        onToggle={() => setQueueOpen((value) => !value)}
        onPause={(id) => void api.downloadPause(id)}
        onResume={(id) => void api.downloadResume(id)}
        onCancel={(id) => void api.downloadCancel(id)}
        onForget={(id) => void api.downloadForget(id)}
        onClearList={() => void clearQueue()}
        onOpenFolder={(id) => void openFolder(id)}
      />
      </div>

      {filtersOpen ? (
        <FilterDrawer
          groups={filterGroups}
          selected={selectedTags}
          expanded={expandedFilters}
          onToggle={toggleTag}
          onExpand={(group) =>
            setExpandedFilters((current) =>
              current.includes(group)
                ? current.filter((name) => name !== group)
                : [...current, group],
            )
          }
          onClear={() => {
            setSelectedTags([]);
            setPage(1);
          }}
          onClose={() => setFiltersOpen(false)}
        />
      ) : null}

      {detailItem ? (
        <DetailsDialog
          item={detailItem}
          details={details}
          loading={detailsLoading}
          error={detailsError}
          downloaded={downloadedIds.has(detailItem.id)}
          onClose={closeDetails}
          onDownload={() => void runDownload(detailItem)}
          onOpenFolder={() => void openFolder(detailItem.id)}
          onRemove={async () => {
            await removeDownload(detailItem.id);
            setDetailItem(null);
          }}
          onRetry={() => void loadDetails(detailItem)}
          onOpenAuthor={handleOpenAuthor}
        />
      ) : null}

      {loginOpen ? (
        <LoginDialog
          status={loginStatus}
          onRefresh={() => void refreshLoginStatus()}
          onClose={() => setLoginOpen(false)}
        />
      ) : null}

      {experimentalOpen ? (
        <ExperimentalDialog
          status={experimental}
          onChange={setExperimental}
          onClose={() => setExperimentalOpen(false)}
        />
      ) : null}

      {installOpen ? (
        <InstallDialog
          info={installInfo}
          onClose={() => setInstallOpen(false)}
          onChanged={() => {
            void api.info().then(setInstallInfo).catch(() => undefined);
            void loadDownloads();
          }}
        />
      ) : null}

      <Notice message={notice} onDismiss={() => setNotice(null)} />
    </>
  );
}
