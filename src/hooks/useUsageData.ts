import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { heatmapFilter } from "../lib/calendar";
import { clearCursorSessionDetailCache } from "../lib/cursorSessionDetailCache";
import { customRangeFilter, humanStatus, rangeFromPreset } from "../lib/format";
import { rangeSnapshot } from "../lib/rangeHistory";
import type {
  ApplicationAnalyticsDto,
  BillingWindowsDto,
  BudgetConfig,
  BudgetStatusDto,
  CodeVolumeSummary,
  CursorAccountUsageDto,
  CursorSessionSummaryDto,
  Filter,
  FilterOptions,
  Grain,
  IngestReport,
  NamedAmount,
  OfficialQuotaDto,
  OverviewDto,
  PriceTable,
  SeriesPoint,
  SessionRow,
  SourceDiagnostic,
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
import { emptyFilter } from "./usage/constants";
import { useAutoRefresh } from "./usage/useAutoRefresh";
import { useCursorAccountRefresh } from "./usage/useCursorAccountRefresh";
import { useIngestOperations } from "./usage/useIngestOperations";
import { useRangeHistory } from "./usage/useRangeHistory";
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
  const [options, setOptions] = useState<FilterOptions>({
    sources: [],
    models: [],
    projects: [],
    providers: [],
  });
  const [overview, setOverview] = useState<OverviewDto | null>(null);
  const [billingWindows, setBillingWindows] = useState<BillingWindowsDto | null>(null);
  const [officialQuota, setOfficialQuota] = useState<OfficialQuotaDto | null>(null);
  const [cursorAccountUsage, setCursorAccountUsage] = useState<CursorAccountUsageDto | null>(null);
  const [previous, setPrevious] = useState<OverviewDto | null>(null);
  const [trend, setTrend] = useState<SeriesPoint[]>([]);
  const [heatmap, setHeatmap] = useState<SeriesPoint[]>([]);
  const [heatmapRange, setHeatmapRange] = useState(() => {
    const window = heatmapFilter(emptyFilter);
    return { from: window.fromDate, to: window.toDate };
  });
  const [grain, setGrain] = useState<Grain>("day");
  const [applicationAnalytics, setApplicationAnalytics] = useState<ApplicationAnalyticsDto | null>(
    null,
  );
  const [models, setModels] = useState<NamedAmount[]>([]);
  const [projects, setProjects] = useState<NamedAmount[]>([]);
  const [providerBreakdown, setProviderBreakdown] = useState<NamedAmount[]>([]);
  const [hydratedViews, setHydratedViews] = useState<Set<View>>(() => new Set());
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [sessionsRevision, setSessionsRevision] = useState(0);
  const [conversationFocus, setConversationFocus] = useState<ConversationFocus | null>(
    () => parseConversationFocus(window.location.hash),
  );
  const [prices, setPrices] = useState<PriceTable>({ prices: [] });
  const [budgetStatus, setBudgetStatus] = useState<BudgetStatusDto | null>(null);
  const [savingBudget, setSavingBudget] = useState(false);
  const [diagnostics, setDiagnostics] = useState<SourceDiagnostic[]>([]);
  const [lastIngestReport, setLastIngestReport] = useState<IngestReport | null>(null);
  const [codeVolume, setCodeVolume] = useState<CodeVolumeSummary | null>(null);
  const [codeVolumeLoading, setCodeVolumeLoading] = useState(() => viewFromHash() === "cursor");
  const [cursorSessionSummary, setCursorSessionSummary] = useState<CursorSessionSummaryDto | null>(
    null,
  );
  const [cursorSessionLoading, setCursorSessionLoading] = useState(
    () => viewFromHash() === "cursor-sessions",
  );
  const [status, setStatus] = useState("正在连接…");
  const [connected, setConnected] = useState(false);
  const [loading, setLoading] = useState(true);
  const [updatedAt, setUpdatedAt] = useState<string | null>(null);

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
    setLoading,
    setOptions,
    setOverview,
    setTrend,
    setModels,
    setProjects,
    setSessions,
    setBillingWindows,
    setOfficialQuota,
    setCursorAccountUsage,
    setBudgetStatus,
    setHeatmap,
    setHeatmapRange,
    setPrevious,
    setApplicationAnalytics,
    setProviderBreakdown,
    setCodeVolume,
    setCodeVolumeLoading,
    setCursorSessionSummary,
    setCursorSessionLoading,
    setPrices,
    setDiagnostics,
    setUpdatedAt,
  });

  const wrappedRefreshViews = useCallback(async () => {
    await refreshViews();
  }, [refreshViews]);

  const { busy, rebuilding, purging, runIngest, runRebuild, runPurgeArchived } =
    useIngestOperations({
      refreshViews: wrappedRefreshViews,
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
        setBudgetStatus(nextStatus);
        setStatus("预算设置已保存");
      } catch (error) {
        setStatus(`预算设置保存失败：${humanStatus(error)}`);
        throw error;
      } finally {
        setSavingBudget(false);
      }
    },
    [],
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
          setLoading(false);
        }
        return runIngestRef.current("启动摄取");
      })
      .catch((error: unknown) => {
        setConnected(false);
        setStatus(humanStatus(error));
        setLoading(false);
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
      setViewScopes((current) =>
        syncSharedFilters(current, next, current[target].preset, target),
      );
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

  return {
    view,
    filter,
    preset,
    options,
    overview,
    billingWindows,
    officialQuota,
    setOfficialQuota,
    cursorAccountUsage,
    cursorAccountAutoRefresh,
    setCursorAccountAutoRefresh,
    cursorAccountRevision,
    refreshCursorAccount,
    previous,
    trend,
    heatmap,
    heatmapRange,
    grain,
    setGrain,
    breakdown:
      view === "provider" ? providerBreakdown : view === "project" ? projects : models,
    applicationAnalytics,
    models,
    projects,
    sessions,
    sessionsRevision,
    conversationFocus,
    prices,
    setPrices,
    budgetStatus,
    savingBudget,
    saveBudget,
    diagnostics,
    lastIngestReport,
    rebuilding,
    purging,
    codeVolume,
    codeVolumeLoading,
    cursorSessionSummary,
    cursorSessionLoading,
    status,
    setStatus,
    connected,
    busy,
    loading,
    viewHasData: hydratedViews.has(view),
    updatedAt,
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
