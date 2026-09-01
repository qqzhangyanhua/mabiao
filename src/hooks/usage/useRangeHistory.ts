import { useCallback, useMemo, useState } from "react";
import {
  popRangeHistory,
  pushRangeHistory,
  sameRange,
  type RangeSnapshot,
} from "../../lib/rangeHistory";

export function useRangeHistory() {
  const [history, setHistory] = useState<RangeSnapshot[]>([]);

  const canGoBack = history.length > 0;

  const pushCurrent = useCallback((current: RangeSnapshot, next: RangeSnapshot): boolean => {
    if (sameRange(current, next)) {
      return false;
    }
    setHistory((hist) => pushRangeHistory(hist, current, next));
    return true;
  }, []);

  const pop = useCallback((): RangeSnapshot | null => {
    const popped = popRangeHistory(history);
    if (!popped.previous) {
      return null;
    }
    setHistory((hist) => {
      const latest = popRangeHistory(hist);
      return latest.previous ? latest.history : hist;
    });
    return popped.previous;
  }, [history]);

  const clear = useCallback(() => {
    setHistory((hist) => (hist.length ? [] : hist));
  }, []);

  return useMemo(
    () => ({ canGoBack, pushCurrent, pop, clear }),
    [canGoBack, clear, pop, pushCurrent],
  );
}
