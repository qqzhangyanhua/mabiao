import { useEffect, useState } from "react";

export interface AsyncState<T> {
  data: T | undefined;
  error: Error | undefined;
  /** 第一次加载或 `load` 换了之后、新结果回来之前。`reload` 期间保留旧数据，不算加载中。 */
  loading: boolean;
  reload: () => void;
}

interface Settled<T> {
  source: () => Promise<T>;
  data?: T;
  error?: Error;
}

function asError(value: unknown): Error {
  return value instanceof Error ? value : new Error(String(value));
}

/** `load` 必须用 `useCallback` 固定住：它变了就当成新请求，旧请求的结果丢弃。 */
export function useAsync<T>(load: () => Promise<T>): AsyncState<T> {
  const [settled, setSettled] = useState<Settled<T> | null>(null);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let cancelled = false;
    load().then(
      (data) => {
        if (!cancelled) setSettled({ source: load, data });
      },
      (error: unknown) => {
        if (!cancelled) setSettled({ source: load, error: asError(error) });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [load, nonce]);

  const current = settled?.source === load ? settled : null;
  return {
    data: current?.data,
    error: current?.error,
    loading: current === null,
    reload: () => setNonce((n) => n + 1),
  };
}

export function useHashRoute(): string {
  const [hash, setHash] = useState(() => window.location.hash);
  useEffect(() => {
    const onChange = () => setHash(window.location.hash);
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);
  return hash;
}
