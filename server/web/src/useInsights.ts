import { useCallback, useMemo, useState } from "react";
import type { ScopeParams } from "./api/client";
import { useAuth } from "./auth";
import { useAsync } from "./hooks";
import { compositionSplits } from "./lib/composition";
import type { UnifiedCostSource } from "./lib/cost";

export function useScopeInsights(scope: ScopeParams | null) {
  const { api } = useAuth();
  const accountId = scope?.accountId;
  const projectId = scope?.projectId;
  const from = scope?.from;
  const to = scope?.to;
  const stable = useMemo<ScopeParams | null>(() => {
    if (accountId === undefined && projectId === undefined && from === undefined && to === undefined) {
      return null;
    }
    return { accountId, projectId, from, to };
  }, [accountId, projectId, from, to]);
  const loadComposition = useCallback(
    () => (stable ? api.composition(stable) : Promise.reject(new Error("日期范围不正确"))),
    [api, stable],
  );
  const composition = useAsync(loadComposition);
  const loadBehavior = useCallback(
    () => (stable ? api.behavior(stable) : Promise.reject(new Error("日期范围不正确"))),
    [api, stable],
  );
  const behavior = useAsync(loadBehavior);
  const [costSource, setCostSource] = useState<UnifiedCostSource | "">("");
  const [model, setModel] = useState("");
  return {
    composition,
    behavior,
    costSource,
    setCostSource,
    model,
    setModel,
    showUnpriced: () => {
      setCostSource("none");
      setModel("");
    },
    scope: stable,
    ...compositionSplits(composition.data),
  };
}
