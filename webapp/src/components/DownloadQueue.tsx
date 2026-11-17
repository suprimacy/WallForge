import type { DownloadJob } from "../types";

interface Props {
  jobs: DownloadJob[];
  open: boolean;
  onToggle: () => void;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onForget: (id: string) => void;
  onClearList: () => void;
  onOpenFolder: (id: string) => void;
}

const ACTIVE_STATES = ["queued", "downloading", "paused"];

export function DownloadQueue({
  jobs,
  open,
  onToggle,
  onPause,
  onResume,
  onCancel,
  onForget,
  onClearList,
  onOpenFolder,
}: Props) {
  const active = jobs.filter((job) => ACTIVE_STATES.includes(job.state));
  const finished = jobs.filter((job) => !ACTIVE_STATES.includes(job.state));
  const clearable = jobs.some((job) => job.state !== "downloading" && job.state !== "paused");

  if (!open) {
    return (
      <button
        type="button"
        className="queue-tab"
        onClick={onToggle}
        aria-label="Open the download queue"
        title="Open the download queue"
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
          <path d="M12 4v11" />
          <path d="M7.5 10.5 12 15l4.5-4.5" />
          <path d="M5 19h14" />
        </svg>
        {active.length ? (
          <span className="queue-tab-count">{active.length}</span>
        ) : null}
      </button>
    );
  }

  return (
    <aside className="queue" aria-label="Download queue">
      <header className="queue-head">
        <h2>Downloads</h2>
        {active.length ? (
          <span className="queue-badge" aria-label={`${active.length} active`}>
            {active.length}
          </span>
        ) : null}
        <div className="queue-head-actions">
          {clearable ? (
            <button
              type="button"
              className="ghost queue-clear"
              onClick={onClearList}
              title="Remove queued and finished downloads from the list"
            >
              Clear list
            </button>
          ) : null}
          <button
            type="button"
            className="icon-btn queue-close"
            onClick={onToggle}
            aria-label="Close the download queue"
            title="Close the download queue"
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
              <path d="M15 5l-7 7 7 7" />
            </svg>
          </button>
        </div>
      </header>

      <div className="queue-scroll">
        {jobs.length === 0 ? (
          <div className="queue-empty">
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
              focusable="false"
            >
              <path d="M12 4v11" />
              <path d="M7.5 10.5 12 15l4.5-4.5" />
              <path d="M5 19h14" />
            </svg>
            <p>No downloads yet</p>
            <span>Press Download on a wallpaper and it shows up here.</span>
          </div>
        ) : null}

        {active.length ? (
          <section className="queue-section" aria-label="In progress">
            {active.map((job) => (
              <QueueRow
                key={job.id}
                job={job}
                onPause={onPause}
                onResume={onResume}
                onCancel={onCancel}
              />
            ))}
          </section>
        ) : null}

        {finished.length ? (
          <section className="queue-section" aria-label="Finished">
            <h3 className="queue-section-title">Finished</h3>
            {finished.map((job) => (
              <QueueRow
                key={job.id}
                job={job}
                onForget={onForget}
                onOpenFolder={onOpenFolder}
              />
            ))}
          </section>
        ) : null}
      </div>
    </aside>
  );
}

interface RowProps {
  job: DownloadJob;
  onPause?: (id: string) => void;
  onResume?: (id: string) => void;
  onCancel?: (id: string) => void;
  onForget?: (id: string) => void;
  onOpenFolder?: (id: string) => void;
}

function QueueRow({
  job,
  onPause,
  onResume,
  onCancel,
  onForget,
  onOpenFolder,
}: RowProps) {
  const running = job.state === "downloading";
  const paused = job.state === "paused";
  const percent = Math.round(job.progress);

  return (
    <article className={`queue-row queue-row-${job.state}`}>
      <div className="queue-row-art">
        {job.img ? (
          <img src={job.img} alt="" loading="lazy" />
        ) : (
          <span className="queue-row-art-fallback" aria-hidden="true">
            {job.title.trim().charAt(0).toUpperCase() || "?"}
          </span>
        )}
        {running ? (
          <span
            className="queue-row-progress"
            style={{ "--fill": `${percent}%` } as React.CSSProperties}
          />
        ) : null}
      </div>

      <div className="queue-row-body">
        <p className="queue-row-title" title={job.title || job.id}>
          {job.title || `Workshop ${job.id}`}
        </p>

        <p className={`queue-row-note queue-row-note-${job.state}`} title={job.message}>
          {running ? `${percent}% · ${job.message}` : job.message}
        </p>

        <div className="queue-row-actions">
          {running && onPause ? (
            <button
              type="button"
              className="queue-btn"
              onClick={() => onPause(job.id)}
              aria-label={`Pause ${job.title || job.id}`}
              title="Pause"
            >
              <svg
                viewBox="0 0 24 24"
                fill="currentColor"
                aria-hidden="true"
                focusable="false"
              >
                <rect x="7" y="5" width="3.6" height="14" rx="1" />
                <rect x="13.4" y="5" width="3.6" height="14" rx="1" />
              </svg>
            </button>
          ) : null}

          {paused && onResume ? (
            <button
              type="button"
              className="queue-btn queue-btn-accent"
              onClick={() => onResume(job.id)}
              aria-label={`Resume ${job.title || job.id}`}
              title="Resume"
            >
              <svg
                viewBox="0 0 24 24"
                fill="currentColor"
                aria-hidden="true"
                focusable="false"
              >
                <path d="M8 5.5v13a1 1 0 0 0 1.53.85l10-6.5a1 1 0 0 0 0-1.7l-10-6.5A1 1 0 0 0 8 5.5Z" />
              </svg>
            </button>
          ) : null}

          {(running || paused || job.state === "queued") && onCancel ? (
            <button
              type="button"
              className="queue-btn"
              onClick={() => onCancel(job.id)}
              aria-label={`Cancel ${job.title || job.id}`}
              title="Cancel"
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
          ) : null}

          {job.state === "done" && onOpenFolder ? (
            <button
              type="button"
              className="queue-btn queue-btn-accent"
              onClick={() => onOpenFolder(job.id)}
              aria-label={`Open the folder for ${job.title || job.id}`}
              title="Open folder"
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
                <path d="M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2.5h8a1.5 1.5 0 0 1 1.5 1.5v8A1.5 1.5 0 0 1 19 19.5H5A1.5 1.5 0 0 1 3.5 18z" />
              </svg>
            </button>
          ) : null}

          {(job.state === "done" ||
            job.state === "failed" ||
            job.state === "cancelled") && onForget ? (
            <button
              type="button"
              className="queue-btn"
              onClick={() => onForget(job.id)}
              aria-label={`Remove ${job.title || job.id} from the list`}
              title="Remove from list"
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
          ) : null}
        </div>
      </div>
    </article>
  );
}
