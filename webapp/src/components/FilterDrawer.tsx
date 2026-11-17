import type { FilterGroup } from "../types";

interface Props {
  groups: FilterGroup[];
  selected: string[];
  expanded: string[];
  onToggle: (tag: string, single: boolean) => void;
  onExpand: (group: string) => void;
  onClear: () => void;
  onClose: () => void;
}

/**
 * The Workshop filters, shown as a drawer over the results.
 *
 * It sits beside the grid instead of above it, so the results never move
 * when a filter is picked. Long groups scroll within the drawer.
 */
export function FilterDrawer({
  groups,
  selected,
  expanded,
  onToggle,
  onExpand,
  onClear,
  onClose,
}: Props) {
  const count = selected.length;

  return (
    <>
      <div className="drawer-backdrop" onClick={onClose} />
      <aside
        className="drawer"
        role="dialog"
        aria-modal="true"
        aria-label="Workshop filters"
      >
        <header className="drawer-head">
          <h2 className="drawer-title">Filters</h2>
          {count ? (
            <span className="drawer-count" aria-label={`${count} filters on`}>
              {count}
            </span>
          ) : null}
          <button
            type="button"
            className="icon-btn drawer-close"
            onClick={onClose}
            aria-label="Close filters"
            title="Close filters"
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
        </header>

        <div className="drawer-body">
          {groups.length === 0 ? (
            <p className="drawer-empty">
              Loading the Workshop filters.
            </p>
          ) : null}

          {groups.map((group) => {
            const isExpanded = expanded.includes(group.name);
            return (
              <section className="filter-group" key={group.name}>
                <h3 className="filter-label">{group.name}</h3>

                {group.type === "checkbox" ? (
                  <>
                    <div
                      className={`filter-options${
                        isExpanded ? " expanded" : ""
                      }`}
                    >
                      {group.tags.map((tag) => (
                        <label className="filter-option" key={tag}>
                          <input
                            type="checkbox"
                            checked={selected.includes(tag)}
                            onChange={() => onToggle(tag, false)}
                          />
                          <span>{tag}</span>
                        </label>
                      ))}
                    </div>
                    {group.tags.length > 6 ? (
                      <button
                        type="button"
                        className="filter-more"
                        onClick={() => onExpand(group.name)}
                      >
                        {isExpanded
                          ? "Show less"
                          : `Show all ${group.tags.length}`}
                      </button>
                    ) : null}
                  </>
                ) : (
                  <select
                    className="filter-select"
                    value={
                      group.tags.find((tag) => selected.includes(tag)) ?? ""
                    }
                    onChange={(event) => {
                      const value = event.target.value;
                      onToggle(value, true);
                    }}
                  >
                    <option value="">Any</option>
                    {group.tags.map((tag) => (
                      <option value={tag} key={tag}>
                        {tag}
                      </option>
                    ))}
                  </select>
                )}
              </section>
            );
          })}
        </div>

        <footer className="drawer-foot">
          <button
            type="button"
            id="clear-filters"
            className="ghost"
            onClick={onClear}
            disabled={count === 0}
          >
            Clear all
          </button>
          <button type="button" className="primary" onClick={onClose}>
            Show {count ? `${count} filtered` : "all"}
          </button>
        </footer>
      </aside>
    </>
  );
}
