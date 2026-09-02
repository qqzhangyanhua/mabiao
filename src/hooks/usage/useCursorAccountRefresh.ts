import { invoke } from "@tauri-apps/api/core";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import { humanStatus } from "../../lib/format";
import type {
  ApplicationAnalyticsDto,
  BillingWindowsDto,
  CursorAccountUsageDto,
  Filter,
  Grain,
  NamedAmount,
  SeriesPoint,
  View,
} from "../../types";
import {
  CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES,
  CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY,
  loadCursorAccountAutoRefresh,
} from "./constants";

const CURSOR_ACCOUNT_VIEWS: View[] = ["overview", "trend", "application", "project"];

type Args = {
  filter: Filter;
  view: View;
  grain: Grain;
  loadedStampsRef: MutableRefObject<Partial<Record<View, string>>>;
  setCursorAccountUsage: Dispatch<SetStateAction<CursorAccountUsageDto | null>>;
  setBillingWindows: Dispatch<SetStateAction<BillingWindowsDto | null>>;
  setTrend: Dispatch<SetStateAction<SeriesPoint[]>>;
  setApplicationAnalytics: Dispatch<SetStateAction<ApplicationAnalyticsDto | null>>;
  setProjects: Dispatch<SetStateAction<NamedAmount[]>>;
};

export function useCursorAccountRefresh({
  filter,
  view,
  grain,
  loadedStampsRef,
  setCursorAccountUsage,
  setBillingWindows,
  setTrend,
  setApplicationAnalytics,
  setProjects,
}: Args) {
  const [autoRefresh, setAutoRefresh] = useState(loadCursorAccountAutoRefresh);
  const [revision, setRevision] = useState(0);
  const [refreshError, setRefreshError] = useState<string | null>(null);
  const inflight = useRef<Promise<void> | null>(null);
  const filterRef = useRef(filter);
  const viewRef = useRef(view);
  const grainRef = useRef(grain);
  const refreshRef = useRef<() => Promise<void>>(() => Promise.resolve());
  const wasEnabledRef = useRef(autoRefresh);

  useEffect(() => {
    filterRef.current = filter;
  }, [filter]);
  useEffect(() => {
    viewRef.current = view;
  }, [view]);
  useEffect(() => {
    grainRef.current = grain;
  }, [grain]);

  const reloadSurfaces = useCallback(async () => {
    const currentFilter = filterRef.current;
    const currentView = viewRef.current;
    const currentGrain = grainRef.current;
    const summary = await invoke<CursorAccountUsageDto>("get_cursor_account_usage", {
      filter: currentFilter,
    });
    setCursorAccountUsage(summary);
    setRevision((value) => value + 1);
    for (const target of CURSOR_ACCOUNT_VIEWS) {
      if (target !== currentView) {
        delete loadedStampsRef.current[target];
      }
    }
    if (currentView === "overview") {
      const [windows, trend, projects] = await Promise.all([
        invoke<BillingWindowsDto>("get_billing_windows", { filter: currentFilter }),
        invoke<SeriesPoint[]>("get_trend", { filter: currentFilter, grain: currentGrain }),
        invoke<NamedAmount[]>("get_breakdown", {
          query: { filter: currentFilter, dimension: "project" },
        }),
      ]);
      setBillingWindows(windows);
      setTrend(trend);
      setProjects(projects);
      return;
    }
    if (currentView === "trend") {
      setTrend(
        await invoke<SeriesPoint[]>("get_trend", {
          filter: currentFilter,
          grain: currentGrain,
        }),
      );
      return;
    }
    if (currentView === "application") {
      setApplicationAnalytics(
        await invoke<ApplicationAnalyticsDto>("get_application_analytics", {
          filter: currentFilter,
          grain: currentGrain,
        }),
      );
      return;
    }
    if (currentView === "project") {
      setProjects(
        await invoke<NamedAmount[]>("get_breakdown", {
          query: { filter: currentFilter, dimension: "project" },
        }),
      );
    }
  }, [
    loadedStampsRef,
    setApplicationAnalytics,
    setBillingWindows,
    setCursorAccountUsage,
    setProjects,
    setTrend,
  ]);

  const refresh = useCallback(async () => {
    if (inflight.current) {
      return inflight.current;
    }
    const run = (async () => {
      try {
        await invoke<CursorAccountUsageDto>("refresh_cursor_account_usage");
        await reloadSurfaces();
        setRefreshError(null);
      } catch (error) {
        setRefreshError(humanStatus(error));
        throw error;
      } finally {
        inflight.current = null;
      }
    })();
    inflight.current = run;
    return run;
  }, [reloadSurfaces]);

  useEffect(() => {
    refreshRef.current = refresh;
  }, [refresh]);

  useEffect(() => {
    try {
      window.localStorage.setItem(
        CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY,
        autoRefresh ? "on" : "off",
      );
    } catch {
      // localStorage 不可用时忽略，仅影响下次启动是否记住选择
    }
    if (!autoRefresh) {
      return;
    }
    const id = window.setInterval(() => {
      refreshRef.current().catch((error: unknown) => {
        setRefreshError(humanStatus(error));
      });
    }, CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES * 60_000);
    return () => window.clearInterval(id);
  }, [autoRefresh]);

  useEffect(() => {
    const wasEnabled = wasEnabledRef.current;
    wasEnabledRef.current = autoRefresh;
    if (autoRefresh && !wasEnabled) {
      refreshRef.current().catch((error: unknown) => {
        setRefreshError(humanStatus(error));
      });
    }
  }, [autoRefresh]);

  return { autoRefresh, setAutoRefresh, revision, refresh, refreshError };
}
