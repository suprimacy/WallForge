import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { ItemDetails, WallpaperItem } from "../types";

interface Props {
  item: WallpaperItem | null;
  details: ItemDetails | null;
  loading: boolean;
  error: string | null;
  downloaded: boolean;
  onClose: () => void;
  onDownload: () => void;
  onOpenFolder: () => void;
  onRemove: () => void;
  onRetry: () => void;
  onOpenAuthor: (author: string, authorUrl: string) => void;
}

const FACT_GROUPS = ["Resolution", "Genre", "Type", "Category", "Age Rating"];

/**
 * Prefers the largest screenshot the Workshop page offers, then a YouTube
 * still, and only then the small animated preview.
 */
function heroSources(details: ItemDetails | null, item: WallpaperItem | null): string[] {
  const sources: string[] = [];
  const main = details?.img ?? "";
  if (details?.images?.length) {
    details.images.forEach((src) => {
      if (src && src !== main) sources.push(src);
    });
  }
  if (details?.video_id) {
    sources.push(`https://i.ytimg.com/vi/${details.video_id}/maxresdefault.jpg`);
    sources.push(`https://i.ytimg.com/vi/${details.video_id}/hqdefault.jpg`);
  }
  if (main) sources.push(main);
  if (item?.img) sources.push(item.img);
  return [...new Set(sources.filter(Boolean))];
}

export function DetailsDialog({
  item,
  details,
  loading,
  error,
  downloaded,
  onClose,
  onDownload,
  onOpenFolder,
  onRemove,
  onRetry,
  onOpenAuthor,
}: Props) {
  const [heroIndex, setHeroIndex] = useState(0);
  const [heroSmall, setHeroSmall] = useState(true);
  const [openError, setOpenError] = useState("");
  const imageRef = useRef<HTMLImageElement | null>(null);
  const blurRef = useRef<HTMLImageElement | null>(null);
  const closeRef = useRef<HTMLButtonElement | null>(null);
  const restoreFocus = useRef<HTMLElement | null>(null);

  const openInBrowser = useCallback(async (url: string) => {
    setOpenError("");
    try {
      const result = await api.openUrl(url);
      if (!result.success) {
        setOpenError(result.message || "That link could not be opened.");
      }
    } catch (err) {
      setOpenError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const sources = useMemo(() => heroSources(details, item), [details, item]);

  useEffect(() => {
    setHeroIndex(0);
    setHeroSmall(true);
  }, [item?.id]);

  useEffect(() => {
    if (!item) return;
    restoreFocus.current = document.activeElement as HTMLElement | null;
    closeRef.current?.focus({ preventScroll: true });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      restoreFocus.current?.focus?.({ preventScroll: true });
    };
    // Only re-run when the dialog opens or switches item, never on re-renders.
  }, [item?.id, onClose]);

  // The full preview is always shown, never cropped and never upscaled:
  // the art is centred at its own height and a blurred copy fills the
  // frame around it.
  const measureHero = useCallback(() => {
    const image = imageRef.current;
    if (!image) return;
    setHeroSmall(true);
    if (blurRef.current) {
      blurRef.current.src = image.currentSrc || image.src;
    }
  }, []);

  useEffect(() => {
    measureHero();
  }, [heroIndex, sources, details, measureHero]);

  if (!item) return null;

  const title = details?.title || item.title || `Workshop ${item.id}`;
  const link = details?.link || item.link || "";
  const authorUrl = details?.author_url?.trim() ?? "";
  const authorName = details?.author && details.author !== "Unknown" ? details.author : "";
  const groups = new Map<string, string[]>();
  (details?.tags ?? []).forEach((tag) => {
    const key = tag.group || "";
    const list = groups.get(key) ?? [];
    list.push(tag.name);
    groups.set(key, list);
  });

  const facts: { label: string; value: string }[] = [];
  const used = new Set<string>();
  FACT_GROUPS.forEach((group) => {
    const values = groups.get(group);
    if (!values) return;
    facts.push({ label: group, value: values.join(", ") });
    used.add(group);
  });

  const extraTags: string[] = [];
  groups.forEach((values, group) => {
    if (used.has(group)) return;
    if (!group) {
      extraTags.push(...values);
      return;
    }
    extraTags.push(`${group}: ${values.join(", ")}`);
  });

  return (
    <div
      className="dialog-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="dialog detail-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="detail-title"
      >
        <div className="detail-scroll">
          <div className={`detail-hero${heroSmall ? " is-small" : ""}`}>
            <img ref={blurRef} className="detail-blur" alt="" aria-hidden="true" />
            {sources.length ? (
              <img
                ref={imageRef}
                className="detail-image"
                alt={`${title} preview`}
                src={sources[heroIndex]}
                onLoad={measureHero}
                onError={() => {
                  if (heroIndex < sources.length - 1) setHeroIndex((value) => value + 1);
                }}
              />
            ) : null}
            <button
              type="button"
              className="icon-btn detail-close"
              id="detail-close"
              aria-label="Close details"
              onClick={onClose}
              ref={closeRef}
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

          <div className="detail-content">
            <div className="detail-head">
              <h2 id="detail-title">{title}</h2>
              {authorName ? (
                authorUrl ? (
                  <button
                    type="button"
                    className="detail-author detail-author-link"
                    onClick={() => authorUrl && onOpenAuthor(authorName, authorUrl)}
                    title={`Show ${authorName}'s wallpapers in the app`}
                  >
                    by {authorName}
                  </button>
                ) : (
                  <p className="detail-author">by {authorName}</p>
                )
              ) : null}
              <p className="detail-rating">
                <span className="detail-stars">{details?.rating ?? ""}</span>
                <span className="detail-rating-count">{details?.rating_count ?? ""}</span>
              </p>
            </div>

            <dl className="detail-facts">
              {facts.map((fact) => (
                <div className="fact" key={fact.label}>
                  <dt>{fact.label}</dt>
                  <dd>{fact.value}</dd>
                </div>
              ))}
            </dl>

            {extraTags.length ? (
              <div className="detail-tags">
                {extraTags.map((tag) => (
                  <span className="detail-tag" key={tag}>
                    {tag}
                  </span>
                ))}
              </div>
            ) : null}

            {details?.description ? (
              <p className="detail-description">{details.description}</p>
            ) : null}

            {details?.partial ? (
              <p className="detail-note" role="status">
                Steam is limiting the item page right now — showing the basics. Try Retry in a minute for the full page.
              </p>
            ) : null}

            <div className="detail-status">
              <p className={`dialog-message${error ? " error" : ""}`} role="status">
                {error ?? (openError || (loading ? "Loading details..." : ""))}
              </p>
              {error || details?.partial ? (
                <button type="button" className="secondary" onClick={onRetry}>
                  Retry
                </button>
              ) : null}
            </div>
          </div>
        </div>

        <div className="dialog-actions detail-actions">
          <div className="detail-actions-left">
            <button type="button" className="ghost" onClick={onClose}>
              Close
            </button>
            {downloaded && link ? (
              <button
                type="button"
                className="ghost"
                onClick={() => void openInBrowser(link)}
                title="Open this wallpaper's Workshop page on Steam"
              >
                Open in browser
              </button>
            ) : null}
          </div>
          <div className="detail-actions-right">
            {!downloaded && link ? (
              <button
                type="button"
                className="ghost"
                onClick={() => void openInBrowser(link)}
                title="Open this wallpaper's Workshop page on Steam"
              >
                Open in browser
              </button>
            ) : null}
            {downloaded ? (
              <>
                <button type="button" className="ghost" onClick={onRemove}>
                  Remove
                </button>
                <button type="button" className="ghost" onClick={onOpenFolder}>
                  Open folder
                </button>
              </>
            ) : (
              <button type="button" className="download-btn detail-download" onClick={onDownload}>
                Download
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
