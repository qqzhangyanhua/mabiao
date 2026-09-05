import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { clearCursorSessionDetailCache } from "../lib/cursorSessionDetailCache";
import { customRangeFilter, humanStatus, rangeFromPreset } from "../lib/format";
import { rangeSnapshot } from "../lib/rangeHistory";
import type {
  BudgetConfig,
  BudgetStatusDto,
  Filter,
  Grain,
  IngestReport,
  View,
  ConversationFocus,
} from "../types";
import {
  hashBelongsToView,
  hashForConversation,
  initialViewScopes,
  isViewFresh,
  parseConversationFocus,
  reconcileLoadedStamps,
  replaceLocationHash,
  scopesEqual,
  syncSharedFilters,
  viewFromHash,
  viewsWarmedBy,
  worktimeHashForDay,
  type ViewScope,
} from "./viewCache";
import { SETTINGS_UNPRICED_ANCHOR } from "../lib/settingsTabs";
import { conversationFocusFromSession } from "../lib/sessionEntryCopy";
import { useAutoRefresh } from "./usage/useAutoRefresh";
import { useCursorAccountRefresh } from "./usage/useCursorAccountRefresh";
import { useIngestOperations } from "./usage/useIngestOperations";
import { useRangeHistory } from "./usage/useRangeHistory";
import { useUsageViewState } from "./usage/useUsageViewState";
import { useViewRefresh } from "./usage/useViewRefresh";

export { viewFromHash, views } from "./viewCache";

export function useUsageData() {
  const didMount = useRef(false);
  const requestGeneration = useRef(0);
  const dataEpoch = useRef(0);
  const loadedStamps = useRef<Partial<Record<View, string>>>({});
  const optionsEpoch = useRef(-1);

  const [view, setView] = useState<View>(viewFromHash);
  const [viewScopes, setViewScopes] = useState<Record<View, ViewScope>>(initialViewScopes);
  const { filter, preset } = viewScopes[view];
  const {
    canGoBack,
    pushCurrent: pushRange,
    pop: popRangeHistoryState,
    clear: clearRangeHistory,
  } = useRangeHistory();
  const {
    state,
    apply,
    setOfficialQuota,
    setPrices,
    setLoading,
    setCursorAccountUsage,
    setBillingWindows,
    setTrend,
    setApplicationAnalytics,
    setProjects,
  } = useUsageViewState();
  const [grain, setGrain] = useState<Grain>("day");
  const [hydratedViews, setHydratedViews] = useState<Set<View>>(() => new Set());
  const [sessionsRevision, setSessionsRevision] = useState(0);
  const [conversationFocus, setConversationFocus] = useState<ConversationFocus | null>(() =>
    parseConversationFocus(window.location.hash),
  );
  const [savingBudget, setSavingBudget] = useState(false);
  const [lastIngestReport, setLastIngestReport] = useState<IngestReport | null>(null);
  const [status, setStatus] = useState("正在连接…");
  const [connected, setConnected] = useState(false);

  const reportError = useCallback((error: unknown) => {
    setStatus(humanStatus(error));
  }, []);

  const markHydrated = useCallback(
    (target: View, nextFilter: Filter, nextPreset: string, scopes: Record<View, ViewScope>) => {
      const epoch = dataEpoch.current;
      const used: ViewScope = { filter: nextFilter, preset: nextPreset };
      loadedStamps.current = reconcileLoadedStamps(
        loadedStamps.current,
        target,
        used,
        scopes,
        grain,
        epoch,
      );
      setHydratedViews((current) => {
        const next = new Set(current);
        next.add(target);
        for (const warmed of viewsWarmedBy(target)) {
          const scope = warmed === target ? used : scopes[warmed];
          if (scopesEqual(scope, used)) {
            next.add(warmed);
          }
        }
        return next;
      });
    },
    [grain],
  );

  const markHydratedForRefresh = useCallback(
    (target: View, nextFilter: Filter, nextPreset: string) => {
      markHydrated(target, nextFilter, nextPreset, viewScopes);
    },
    [markHydrated, viewScopes],
  );

  const { refreshViews, refreshTrend } = useViewRefresh({
    view,
    filter,
    preset,
    grain,
    hydratedViews,
    requestGenerationRef: requestGeneration,
    dataEpochRef: dataEpoch,
    loadedStampsRef: loadedStamps,
    optionsEpochRef: optionsEpoch,
    markHydrated: markHydratedForRefresh,
    apply,
  });

  const { busy, rebuilding, purging, runIngest, runRebuild, runPurgeArchived } =
    useIngestOperations({
      refreshViews,
      dataEpochRef: dataEpoch,
      requestGenerationRef: requestGeneration,
      setSessionsRevision,
      setLastIngestReport,
      setStatus,
      setLoading,
    });

  const runIngestWithCacheClear = useCallback(
    async (label: string) => {
      clearCursorSessionDetailCache();
      await runIngest(label);
    },
    [runIngest],
  );

  const runRebuildWithCacheClear = useCallback(
    async (source: string | null) => {
      clearCursorSessionDetailCache();
      await runRebuild(source);
    },
    [runRebuild],
  );

  const { autoRefresh, setAutoRefresh } = useAutoRefresh(runIngestWithCacheClear, reportError);
  const {
    autoRefresh: cursorAccountAutoRefresh,
    setAutoRefresh: setCursorAccountAutoRefresh,
    revision: cursorAccountRevision,
    refresh: refreshCursorAccount,
    refreshError: cursorAccountRefreshError,
  } = useCursorAccountRefresh({
    filter,
    view,
    grain,
    loadedStampsRef: loadedStamps,
    setCursorAccountUsage,
    setBillingWindows,
    setTrend,
    setApplicationAnalytics,
    setProjects,
  });

  const saveBudget = useCallback(
    async (config: BudgetConfig) => {
      setSavingBudget(true);
      try {
        await invoke("save_budget", { config });
        const nextStatus = await invoke<BudgetStatusDto>("get_budget_status");
        apply({ budgetStatus: nextStatus });
        setStatus("预算设置已保存");
      } catch (error) {
        setStatus(`预算设置保存失败：${humanStatus(error)}`);
        throw error;
      } finally {
        setSavingBudget(false);
      }
    },
    [apply],
  );

  const runIngestRef = useRef(runIngestWithCacheClear);
  useEffect(() => {
    runIngestRef.current = runIngestWithCacheClear;
  }, [runIngestWithCacheClear]);

  useEffect(() => {
    invoke<string>("ping")
      .then(async () => {
        setConnected(true);
        setStatus("正在加载缓存…");
        try {
          await refreshViews();
        } catch (error: unknown) {
          reportError(error);
          apply({ loading: false });
        }
        return runIngestRef.current("启动摄取");
      })
      .catch((error: unknown) => {
        setConnected(false);
        setStatus(humanStatus(error));
        apply({ loading: false });
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 只在启动时拉一次缓存并后台摄取
  }, []);

  useEffect(() => {
    if (!didMount.current) {
      didMount.current = true;
      return;
    }
    if (isViewFresh(loadedStamps.current, view, filter, preset, grain, dataEpoch.current)) {
      return;
    }
    refreshViews().catch(reportError);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 热缓存命中则不重拉
  }, [view]);

  const didMountGrain = useRef(false);
  useEffect(() => {
    if (!didMountGrain.current) {
      didMountGrain.current = true;
      return;
    }
    refreshTrend().catch(reportError);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 只需在 grain 变化时触发
  }, [grain]);

  const openConversations = useCallback((session?: { id: string; source: string }) => {
    const focus = conversationFocusFromSession(session);
    setConversationFocus(focus);
    setView("conversations");
    replaceLocationHash(
      focus ? hashForConversation(focus.source, focus.session_id) : "conversations",
    );
  }, []);

  const openWorktime = useCallback((day: string) => {
    const hash = worktimeHashForDay(day);
    if (!hash) {
      return;
    }
    replaceLocationHash(hash);
    setView("worktime");
  }, []);

  const openUnpricedDiagnosis = useCallback(() => {
    setView("settings");
    replaceLocationHash(SETTINGS_UNPRICED_ANCHOR);
  }, []);

  const clearConversationFocus = useCallback(() => {
    setConversationFocus(null);
  }, []);

  const navigate = useCallback((next: View) => {
    setView(next);
    if (hashBelongsToView(window.location.hash, next)) {
      return;
    }
    replaceLocationHash(next);
  }, []);

  const applyScope = useCallback(
    (nextPreset: string, explicitRange?: { from: string | null; to: string | null }) => {
      const range = explicitRange ?? rangeFromPreset(nextPreset);
      const nextFilter = { ...filter, ...range };
      setViewScopes((current) => syncSharedFilters(current, nextFilter, nextPreset, view));
      refreshViews(nextFilter, nextPreset).catch(reportError);
    },
    [filter, view, refreshViews, reportError],
  );

  const applyPreset = useCallback(
    (next: string, explicitRange?: { from: string | null; to: string | null }) => {
      clearRangeHistory();
      applyScope(next, explicitRange);
    },
    [applyScope, clearRangeHistory],
  );

  const drillRange = useCallback(
    (from: string, to: string) => {
      const range = customRangeFilter(from, to);
      const pushed = pushRange(
        rangeSnapshot(preset, filter.from, filter.to),
        rangeSnapshot("custom", range.from, range.to),
      );
      if (!pushed) {
        return;
      }
      applyScope("custom", range);
    },
    [applyScope, filter.from, filter.to, preset, pushRange],
  );

  const popRange = useCallback(() => {
    const previous = popRangeHistoryState();
    if (!previous) {
      return;
    }
    applyScope(previous.preset, { from: previous.from, to: previous.to });
  }, [applyScope, popRangeHistoryState]);

  const applyViewFilter = useCallback(
    (target: View, next: Filter) => {
      setViewScopes((current) => syncSharedFilters(current, next, current[target].preset, target));
      if (target === view) {
        refreshViews(next).catch(reportError);
      }
    },
    [view, refreshViews, reportError],
  );

  const applyFilter = useCallback(
    (next: Filter) => {
      applyViewFilter(view, next);
    },
    [applyViewFilter, view],
  );

  const { providerBreakdown, ...viewFields } = state;

  return {
    view,
    filter,
    preset,
    ...viewFields,
    setOfficialQuota,
    cursorAccountAutoRefresh,
    setCursorAccountAutoRefresh,
    cursorAccountRevision,
    cursorAccountRefreshError,
    refreshCursorAccount,
    grain,
    setGrain,
    breakdown:
      view === "provider"
        ? providerBreakdown
        : view === "project"
          ? viewFields.projects
          : viewFields.models,
    sessionsRevision,
    conversationFocus,
    setPrices,
    savingBudget,
    saveBudget,
    lastIngestReport,
    rebuilding,
    purging,
    status,
    setStatus,
    connected,
    busy,
    viewHasData: hydratedViews.has(view),
    autoRefresh,
    setAutoRefresh,
    navigate,
    applyPreset,
    drillRange,
    popRange,
    canGoBack,
    applyFilter,
    openConversations,
    openWorktime,
    openUnpricedDiagnosis,
    clearConversationFocus,
    runIngest: runIngestWithCacheClear,
    runRebuild: runRebuildWithCacheClear,
    runPurgeArchived,
    reportError,
  };
}
