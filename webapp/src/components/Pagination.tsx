interface Props {
  page: number;
  totalPages: number;
  onPage: (page: number) => void;
  className?: string;
}

/**
 * The page controls used above and below the results grid, so the two
 * never drift out of sync.
 */
export function Pagination({ page, totalPages, onPage, className }: Props) {
  return (
    <div className={`pagination${className ? ` ${className}` : ""}`}>
      <div className="page-controls">
        <button
          type="button"
          className="btn-page"
          disabled={page <= 1}
          onClick={() => onPage(Math.max(1, page - 1))}
          aria-label="Previous page"
        >
          <span>Prev</span>
        </button>
        <label className="page-jump">
          Page
          <input
            className="page-input"
            type="number"
            min={1}
            max={totalPages || undefined}
            inputMode="numeric"
            aria-label="Page number"
            value={page}
            onChange={(event) => {
              const value = Number(event.target.value);
              if (Number.isFinite(value) && value > 0) onPage(value);
            }}
          />
          of <span className="page-total">{totalPages || "?"}</span>
        </label>
        <button
          type="button"
          className="btn-page"
          disabled={!totalPages || page >= totalPages}
          onClick={() => onPage(page + 1)}
          aria-label="Next page"
        >
          <span>Next</span>
        </button>
      </div>
    </div>
  );
}
