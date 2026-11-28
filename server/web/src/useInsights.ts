import { useMemo, useState } from "react";
import type { ScopeParams } from "./api/client";
import type { BehaviorReport } from "./api/types";
import { useAuth } from "./auth";
import { useAsync, type AsyncState } from "./hooks";
import { compositionSplits } from "./lib/composition";
import type { UnifiedCostSource } from "./lib/cost";

/** 按值稳定 scope 引用，避免父组件每次渲染新建对象触发重复请求。 */
function useStableScope(scope: ScopeParams | null): ScopeParams | null {
  const active = scope !== null;
  const accountId = scope?.accountId;
  const projectId = scope?.projectId;
  const from = scope?.from;
  const to = scope?.to;
  return useMemo<ScopeParams | null>(
    () => (active ? { accountId, projectId, from, to } : null),
    [active, accountId, projectId, from, to],
  );
}

/** 只拉 `/api/v1/usage/composition` 与明细筛选状态，不触发 `/api/v1/sessions/behavior`。 */
export function useScopeComposition(scope: ScopeParams | null) {
  const { api } = useAuth();
  const stable = useStableScope(scope);
  const loadComposition = useMemo(
    () => (stable ? () => api.composition(stable) : null),
    [api, stable],
  );
  const composition = useAsync(loadComposition);
  const [costSource, setCostSource] = useState<UnifiedCostSource | "">("");
  const [model, setModel] = useState("");
  return {
    composition,
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

/** 只拉 `/api/v1/sessions/behavior`，给真正渲染行为面板的页面用。 */
export function useScopeBehavior(scope: ScopeParams | null): AsyncState<BehaviorReport> {
  const { api } = useAuth();
  const stable = useStableScope(scope);
  const loadBehavior = useMemo(
    () => (stable ? () => api.behavior(stable) : null),
    [api, stable],
  );
  return useAsync(loadBehavior);
}
