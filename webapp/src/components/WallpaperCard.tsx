import { useEffect, useRef } from "react";
import type { WallpaperItem } from "../types";
import { useThumbQuality } from "../hooks";

interface Props {
  item: WallpaperItem;
  downloaded: boolean;
  busy: boolean;
  busyLabel: string;
  onOpen: () => void;
  onDownload: () => void;
  onOpenFolder?: () => void;
  onRemove?: () => void;
  children?: React.ReactNode;
}

export function WallpaperCard({
  item,
  downloaded,
  busy,
  busyLabel,
  onOpen,
  onDownload,
  onOpenFolder,
  onRemove,
  children,
}: Props) {
  const observe = useThumbQuality();
  const thumbRef = useRef<HTMLImageElement | null>(null);

  useEffect(() => {
    return observe(thumbRef.current);
  }, [observe, item.img]);

  const stop = (event: React.MouseEvent) => event.stopPropagation();

  return (
    <article className="card clickable" onClick={onOpen}>
      <button
        type="button"
        className="thumb-wrap"
        aria-label={`View details for ${item.title || item.id}`}
        onClick={(event) => {
          stop(event);
          onOpen();
        }}
      >
        <img className="thumb-blur" alt="" aria-hidden="true" loading="lazy" />
        <img
          ref={thumbRef}
          className="thumb"
          alt={item.title || ""}
          loading="lazy"
          src={item.img}
        />
        <span className="card-caption">
          <span className="card-title">{item.title || item.id}</span>
          {item.author ? (
            <span className="card-author">by {item.author}</span>
          ) : null}
        </span>
        {item.rating ? <span className="rating">{item.rating}</span> : null}
      </button>

      <div className="card-body">
        {children ? <div className="card-note">{children}</div> : null}
        <div className="actions">
          {downloaded ? (
            <>
              {onOpenFolder ? (
                <button
                  type="button"
                  className="card-action"
                  onClick={(event) => {
                    stop(event);
                    onOpenFolder();
                  }}
                >
                  Open folder
                </button>
              ) : null}
              {onRemove ? (
                <button
                  type="button"
                  className="card-action"
                  onClick={(event) => {
                    stop(event);
                    onRemove();
                  }}
                >
                  Remove
                </button>
              ) : null}
            </>
          ) : (
            <button
              type="button"
              className="download-btn"
              disabled={busy}
              onClick={(event) => {
                stop(event);
                onDownload();
              }}
            >
              {busy ? busyLabel : "Download"}
            </button>
          )}
        </div>
      </div>
    </article>
  );
}
