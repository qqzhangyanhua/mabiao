import {
  useCallback,
  useMemo,
  useReducer,
  type Dispatch,
  type SetStateAction,
} from "react";
import { heatmapFilter } from "../../lib/calendar";
import type {
  ApplicationAnalyticsDto,
  BillingWindowsDto,
  BudgetStatusDto,
  CodeVolumeSummary,
  CursorAccountUsageDto,
  CursorSessionSummaryDto,
  FilterOptions,
  NamedAmount,
  OfficialQuotaDto,
  OverviewDto,
  PriceTable,
  SeriesPoint,
  SessionRow,
  SourceDiagnostic,
  View,
} from "../../types";
import { viewFromHash } from "../viewCache";
import { emptyFilter } from "./constants";

export type UsageViewState = {
  options: FilterOptions;
  overview: OverviewDto | null;
  billingWindows: BillingWindowsDto | null;
  officialQuota: OfficialQuotaDto | null;
  cursorAccountUsage: CursorAccountUsageDto | null;
  previous: OverviewDto | null;
  trend: SeriesPoint[];
  heatmap: SeriesPoint[];
  heatmapRange: { from: string; to: string };
  applicationAnalytics: ApplicationAnalyticsDto | null;
  models: NamedAmount[];
  projects: NamedAmount[];
  providerBreakdown: NamedAmount[];
  sessions: SessionRow[];
  prices: PriceTable;
  budgetStatus: BudgetStatusDto | null;
  diagnostics: SourceDiagnostic[];
  codeVolume: CodeVolumeSummary | null;
  codeVolumeLoading: boolean;
  cursorSessionSummary: CursorSessionSummaryDto | null;
  cursorSessionLoading: boolean;
  loading: boolean;
  updatedAt: string | null;
};

export type UsageViewPatch = Partial<UsageViewState>;

type UsageViewAction = UsageViewPatch | ((state: UsageViewState) => UsageViewPatch);

export function createInitialUsageViewState(view: View): UsageViewState {
  const heat = heatmapFilter(emptyFilter);
  return {
    options: { sources: [], models: [], projects: [], providers: [] },
    overview: null,
    billingWindows: null,
    officialQuota: null,
    cursorAccountUsage: null,
    previous: null,
    trend: [],
    heatmap: [],
    heatmapRange: { from: heat.fromDate, to: heat.toDate },
    applicationAnalytics: null,
    models: [],
    projects: [],
    providerBreakdown: [],
    sessions: [],
    prices: { prices: [] },
    budgetStatus: null,
    diagnostics: [],
    codeVolume: null,
    codeVolumeLoading: view === "cursor",
    cursorSessionSummary: null,
    cursorSessionLoading: view === "cursor-sessions",
    loading: true,
    updatedAt: null,
  };
}

export function usageViewReducer(state: UsageViewState, patch: UsageViewPatch): UsageViewState {
  return { ...state, ...patch };
}

function reduceUsageView(state: UsageViewState, action: UsageViewAction): UsageViewState {
  const patch = typeof action === "function" ? action(state) : action;
  return usageViewReducer(state, patch);
}

function bindField<K extends keyof UsageViewState>(
  dispatch: Dispatch<UsageViewAction>,
  key: K,
): Dispatch<SetStateAction<UsageViewState[K]>> {
  return (action) => {
    dispatch((state) => {
      const current = state[key];
      const next = typeof action === "function" ? action(current) : action;
      const patch: UsageViewPatch = {};
      patch[key] = next;
      return patch;
    });
  };
}

export function useUsageViewState() {
  const [state, dispatch] = useReducer(
    reduceUsageView,
    viewFromHash(),
    createInitialUsageViewState,
  );

  const apply = useCallback((patch: UsageViewPatch) => {
    dispatch(patch);
  }, []);

  const setters = useMemo(
    () => ({
      setOfficialQuota: bindField(dispatch, "officialQuota"),
      setPrices: bindField(dispatch, "prices"),
      setLoading: bindField(dispatch, "loading"),
      setCursorAccountUsage: bindField(dispatch, "cursorAccountUsage"),
      setBillingWindows: bindField(dispatch, "billingWindows"),
      setTrend: bindField(dispatch, "trend"),
      setApplicationAnalytics: bindField(dispatch, "applicationAnalytics"),
      setProjects: bindField(dispatch, "projects"),
    }),
    [],
  );

  return { state, apply, ...setters };
}
