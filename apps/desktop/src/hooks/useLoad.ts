import { useEffect, useState } from "react";

import { errorMessage } from "../lib/api";

export interface Loaded<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
}

interface Result<T> {
  identity: readonly unknown[];
  refresh: string | number;
  data: T | null;
  error: string | null;
}

function same(a: readonly unknown[], b: readonly unknown[]): boolean {
  return a.length === b.length && a.every((v, i) => Object.is(v, b[i]));
}

/**
 * Load data for `identity` (e.g. `[repositoryPath]`) and reload whenever
 * `refresh` changes. While reloading the same identity, the previous data
 * stays visible; data for a different identity is never shown, so switching
 * repositories cannot briefly display the wrong report. Results of stale
 * requests are discarded.
 */
export function useLoad<T>(
  load: (() => Promise<T>) | null,
  identity: readonly unknown[],
  refresh: string | number = 0,
): Loaded<T> {
  const [result, setResult] = useState<Result<T> | null>(null);
  const enabled = load !== null;

  useEffect(() => {
    if (!load) return;
    let current = true;
    const requested = { identity, refresh };
    load().then(
      (data) => {
        if (current) setResult({ ...requested, data, error: null });
      },
      (e: unknown) => {
        if (current) setResult({ ...requested, data: null, error: errorMessage(e) });
      },
    );
    return () => {
      current = false;
    };
    // `identity` and `refresh` fully describe what to load.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, refresh, ...identity]);

  if (!enabled) return { data: null, error: null, loading: false };
  const sameIdentity = result !== null && same(result.identity, identity);
  const fresh = sameIdentity && result.refresh === refresh;
  return {
    data: sameIdentity ? result.data : null,
    error: fresh ? result.error : null,
    loading: !fresh,
  };
}
