import { useEffect, useState } from "react";

const DEFAULT_TICK_MS = 30_000;

/** 按间隔刷新的 `Date.now()`，给相对时间文案用。 */
export function useTickingNow(intervalMs = DEFAULT_TICK_MS): number {
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return nowMs;
}