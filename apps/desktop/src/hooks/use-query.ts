import { useCallback, useEffect, useRef, useState } from "react";

export type QueryOptions<T = unknown> = {
  /**
   * Re-fetch every this many ms while the window is visible (off when unset). Background refreshes are silent: the
   * current `data` stays on screen, `loading` is left alone and a failure only sets `error`. Polling pauses while the
   * document is hidden and fetches right away when it becomes visible again; a tick is skipped while a fetch is in flight.
   */
  refreshInterval?: number;
  /**
   * Whether a result has nothing to show (the page renders its empty state). Defaults to `isEmpty`; pass one when the
   * result carries lists that aren't the content, e.g. `{ apps, servers }` is empty when `servers` is.
   */
  empty?: (data: T) => boolean;
};

/** Polling intervals (ms): `status` for the gateway state, `live` for call stats, `config` for lists that change only when edited. */
export const REFRESH = { status: 3_000, live: 5_000, config: 20_000 } as const;

/**
 * Minimum time the placeholders stay up, so a fast local fetch reads as a transition instead of a flash: on mount (a
 * tab switch remounts the page), when an empty page gets content, and for a manual `refresh()`, which gets a little
 * longer so it reads as feedback.
 */
const MIN_MOUNT_MS = 200;
const MIN_REFRESH_MS = 400;

const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

/** Nothing to show: null, an empty list, or an object whose lists (it has at least one) are all empty. */
export function isEmpty(v: unknown): boolean {
  if (v == null) return true;
  if (Array.isArray(v)) return v.length === 0;
  if (typeof v !== "object") return false;
  const lists = Object.values(v).filter(Array.isArray);
  return lists.length > 0 && lists.every((l) => l.length === 0);
}

/**
 * The last result of each query (by function source + deps) while it was empty, kept across restarts, so opening an
 * empty page shows its empty state right away instead of skeletons, and refetches quietly.
 */
const EMPTY_STORE = "owo-empty-queries";
const emptyResults: Map<string, unknown> = (() => {
  try {
    return new Map(Object.entries(JSON.parse(localStorage.getItem(EMPTY_STORE) ?? "{}")));
  } catch {
    return new Map();
  }
})();

function rememberEmpty(key: string, result: unknown | undefined) {
  if (result === undefined ? !emptyResults.has(key) : emptyResults.has(key) && same(emptyResults.get(key), result)) return;
  if (result === undefined) emptyResults.delete(key);
  else emptyResults.set(key, result);
  try {
    localStorage.setItem(EMPTY_STORE, JSON.stringify(Object.fromEntries(emptyResults)));
  } catch {
    // Storage full or unavailable: the in-memory copy still covers this session.
  }
}

function queryKey(fn: () => unknown, deps: unknown[]): string {
  try {
    return fn.toString() + JSON.stringify(deps);
  } catch {
    return fn.toString();
  }
}

/** `true` when both serialize the same, so unchanged poll results keep the previous object (no re-render, no chart re-animation). */
function same(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  try {
    return JSON.stringify(a) === JSON.stringify(b);
  } catch {
    return false;
  }
}

/**
 * Loads `fn()` on mount and whenever `deps` change; `reload()` fetches again. `fn` is read at call time, so a timer tick
 * always uses the current arguments (filters). Responses land only when they're the newest request, so a slow fetch
 * can't overwrite the result of a later one after a filter change.
 *
 * `data` is never cleared by a fetch: a `reload()` or a `deps` change keeps the current result on screen until the new
 * one lands. Two flags tell the two situations apart so a page can show skeletons for the one and nothing for the other:
 * - `loading`: a fetch is pending and there is no `data` yet (first load) — render placeholders in the final layout;
 * - `refreshing`: a fetch is pending while `data` is on screen (manual reload, filter change) — keep the content mounted.
 * Neither is set by the silent interval polls.
 *
 * Skeletons are held for a minimum time — `MIN_MOUNT_MS` after mount and when an empty result turns into content,
 * `MIN_REFRESH_MS` for a `refresh()` (the header refresh button, which also spins its icon and ignores clicks meanwhile)
 * — by reporting `data: undefined` and `loading`. An empty result is never held: there is nothing to reveal.
 */
export function useQuery<T>(fn: () => Promise<T>, deps: unknown[] = [], { refreshInterval, empty = isEmpty }: QueryOptions<T> = {}) {
  const key = queryKey(fn, deps);
  const [data, setData] = useState<T | undefined>(() => emptyResults.get(key) as T | undefined);
  const keyRef = useRef(key);
  keyRef.current = key;
  const emptyRef = useRef(empty);
  emptyRef.current = empty;
  /** Whether what is on screen now is an empty result; content arriving after it gets the reveal skeleton. */
  const shownEmpty = useRef(data !== undefined);
  const [error, setError] = useState<string>();
  /** A non-silent fetch (mount, `deps` change, `reload()`) is pending. */
  const [pending, setPending] = useState(data === undefined);
  const latest = useRef(fn);
  latest.current = fn;
  /** Bumped per fetch (and on unmount); a response is applied only when its number is still the newest. */
  const seq = useRef(0);
  const inFlight = useRef(false);

  const [mounting, setMounting] = useState(true);
  const mountingRef = useRef(true);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      mountingRef.current = false;
      setMounting(false);
    }, MIN_MOUNT_MS);
    return () => window.clearTimeout(timer);
  }, []);

  const [revealing, setRevealing] = useState(false);
  const revealTimer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(revealTimer.current), []);

  const [manual, setManual] = useState(false);
  const manualRef = useRef(false);

  const run = useCallback(async (silent: boolean) => {
    const id = ++seq.current;
    inFlight.current = true;
    if (!silent) setPending(true);
    try {
      const result = await latest.current();
      if (id !== seq.current) return;
      const isNone = emptyRef.current(result);
      rememberEmpty(keyRef.current, isNone ? result : undefined);
      // An empty page getting content goes through the skeleton too (mount and refresh already hold it).
      if (shownEmpty.current && !isNone && !mountingRef.current && !manualRef.current) {
        setRevealing(true);
        window.clearTimeout(revealTimer.current);
        revealTimer.current = window.setTimeout(() => setRevealing(false), MIN_MOUNT_MS);
      }
      shownEmpty.current = isNone;
      setData((prev) => (prev !== undefined && same(prev, result) ? prev : result));
      setError(undefined);
    } catch (e) {
      if (id !== seq.current) return;
      setError(String(e));
    } finally {
      // A newer request owns the flags now when the numbers differ.
      if (id === seq.current) {
        inFlight.current = false;
        setPending(false);
      }
    }
  }, []);

  const reload = useCallback(() => run(false), [run]);

  const refresh = useCallback(async () => {
    if (manualRef.current) return;
    manualRef.current = true;
    setManual(true);
    try {
      await Promise.all([run(false), sleep(MIN_REFRESH_MS)]);
    } finally {
      manualRef.current = false;
      setManual(false);
    }
  }, [run]);

  // A remount that starts from a remembered empty result revalidates quietly (no loading, no spinning icon).
  const quietStart = useRef(data !== undefined);
  useEffect(() => {
    void run(quietStart.current);
    quietStart.current = false;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  // Drop whatever is still in flight once unmounted.
  useEffect(
    () => () => {
      seq.current++;
    },
    [],
  );

  useEffect(() => {
    if (!refreshInterval || refreshInterval <= 0) return;
    let timer: number | undefined;
    const tick = () => {
      if (!inFlight.current) void run(true);
    };
    const start = () => {
      if (timer === undefined) timer = window.setInterval(tick, refreshInterval);
    };
    const stop = () => {
      if (timer !== undefined) {
        window.clearInterval(timer);
        timer = undefined;
      }
    };
    const onVisibility = () => {
      if (document.visibilityState === "visible") {
        tick();
        start();
      } else {
        stop();
      }
    };
    if (document.visibilityState === "visible") start();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [refreshInterval, run]);

  const held = (mounting || manual || revealing) && !(data !== undefined && empty(data));
  const loading = held || (pending && data === undefined);
  const refreshing = manual || (pending && data !== undefined);
  return { data: held ? undefined : data, error, loading, refreshing, reload, refresh };
}
