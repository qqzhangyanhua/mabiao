import { useCallback, useEffect, useRef, useState } from "react";
import { humanStatus } from "./format";

export type AsyncLoadState = "loading" | "error";

export function useKeyedAsyncLoad<Key extends string | number>() {
  const [states, setStates] = useState<Partial<Record<Key, AsyncLoadState>>>({});
  const [errors, setErrors] = useState<Partial<Record<Key, string>>>({});
  const mounted = useRef(true);
  const inFlight = useRef(new Set<Key>());

  useEffect(() => {
    const activeRequests = inFlight.current;
    mounted.current = true;
    return () => {
      mounted.current = false;
      activeRequests.clear();
    };
  }, []);

  const run = useCallback(
    async <Result,>(key: Key, task: () => Promise<Result>, onSuccess: (result: Result) => void) => {
      if (inFlight.current.has(key)) {
        return;
      }
      inFlight.current.add(key);
      setStates((current) => ({ ...current, [key]: "loading" }));
      setErrors((current) => {
        const next = { ...current };
        delete next[key];
        return next;
      });
      try {
        const result = await task();
        if (!mounted.current) {
          return;
        }
        onSuccess(result);
        setStates((current) => {
          const next = { ...current };
          delete next[key];
          return next;
        });
      } catch (error) {
        if (mounted.current) {
          setStates((current) => ({ ...current, [key]: "error" }));
          setErrors((current) => ({ ...current, [key]: humanStatus(error) }));
        }
      } finally {
        inFlight.current.delete(key);
      }
    },
    [],
  );

  return { states, errors, run };
}
