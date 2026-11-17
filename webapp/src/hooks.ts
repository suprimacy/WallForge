import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";

/** Keeps a value in localStorage, falling back when storage is unavailable. */
export function useStoredValue<T>(
  key: string,
  initial: T,
): [T, Dispatch<SetStateAction<T>>] {
  const [value, setValue] = useState<T>(() => {
    try {
      const raw = window.localStorage.getItem(key);
      return raw === null ? initial : (JSON.parse(raw) as T);
    } catch {
      return initial;
    }
  });

  const store = useCallback<Dispatch<SetStateAction<T>>>(
    (next) => {
      setValue((current) => {
        const resolved =
          typeof next === "function"
            ? (next as (previous: T) => T)(current)
            : next;
        try {
          window.localStorage.setItem(key, JSON.stringify(resolved));
        } catch {
          /* storage is not available */
        }
        return resolved;
      });
    },
    [key],
  );

  return [value, store];
}

/**
 * Keeps a preview at its native pixel size when it cannot fill the frame.
 *
 * Steam's previews are often small animated GIFs. Stretching one to the card
 * size makes it look blurry, so it is shown at native size over a blurred
 * copy of itself instead.
 */
export function useThumbQuality() {
  const apply = useCallback((img: HTMLImageElement | null) => {
    if (!img) return;
    const wrap = img.closest(".thumb-wrap") as HTMLElement | null;
    if (!wrap || !img.naturalWidth || !wrap.clientWidth) return;
    const needed = wrap.clientWidth * (window.devicePixelRatio || 1);
    const small = img.naturalWidth < needed;
    wrap.classList.toggle("is-small", small);
    if (!small) return;
    const blur = wrap.querySelector(".thumb-blur") as HTMLImageElement | null;
    if (blur && !blur.getAttribute("src")) {
      blur.src = img.currentSrc || img.src;
    }
  }, []);

  const observe = useCallback(
    (img: HTMLImageElement | null) => {
      if (!img) return;
      if (img.complete) {
        apply(img);
        return;
      }
      const onLoad = () => apply(img);
      img.addEventListener("load", onLoad, { once: true });
      return () => img.removeEventListener("load", onLoad);
    },
    [apply],
  );

  useEffect(() => {
    const onResize = () => {
      document
        .querySelectorAll<HTMLImageElement>(".thumb-wrap .thumb")
        .forEach((img) => apply(img));
    };
    let timer = 0;
    const handler = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(onResize, 150);
    };
    window.addEventListener("resize", handler);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("resize", handler);
    };
  }, [apply]);

  return observe;
}

/** Runs an async loader and tracks its state. */
export function useAsync<T>(
  loader: () => Promise<T>,
  deps: unknown[],
): { data: T | null; error: string | null; loading: boolean; reload: () => void } {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    loader()
      .then((result) => {
        if (!cancelled && mounted.current) {
          setData(result);
          setLoading(false);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled && mounted.current) {
          setError(err instanceof Error ? err.message : String(err));
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick]);

  const reload = useCallback(() => setTick((value) => value + 1), []);
  return { data, error, loading, reload };
}
