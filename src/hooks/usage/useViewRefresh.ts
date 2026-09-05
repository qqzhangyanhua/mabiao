import { invoke } from "@tauri-apps/api/core";
import { useCallback, type MutableRefObject } from "react";
import { heatmapFilter } from "../../lib/calendar";
import { previousFilter } from "../../lib/format";
import type {
  ApplicationAnalyticsDto,
  BillingWindowsDto,
  BudgetStatusDto,
  CodeVolumeSummary,
  CursorAccountUsageDto,
  CursorSessionSummaryDto,
  Filter,
  FilterOptions,
  Grain,
  NamedAmount,
  OfficialQuotaDto,
  OverviewDto,
  PriceTable,
  SeriesPoint,
  SessionRow,
  SourceDiagnostic,
  View,
} from "../../types";
import { isViewFresh, viewStamp } from "../viewCache";
import type { UsageViewPatch } from "./useUsageViewState";

type ViewRefreshArgs = {
  view: View;
  filter: Filter;
  preset: string;
  grain: Grain;
  hydratedViews: Set<View>;
  requestGenerationRef: MutableRefObject<number>;
  dataEpochRef: MutableRefObject<number>;
  loadedStampsRef: MutableRefObject<Partial<Record<View, string>>>;
  optionsEpochRef: MutableRefObject<number>;
  markHydrated: (target: View, nextFilter: Filter, nextPreset: string) => void;
  apply: (patch: UsageViewPatch) => void;
};

export function useViewRefresh(args: ViewRefreshArgs) {
  const {
    view,
    filter,
    preset,
    grain,
    hydratedViews,
    requestGenerationRef,
    dataEpochRef,
    loadedStampsRef,
    optionsEpochRef,
    markHydrated,
    apply,
  } = args;

  const refreshViews = useCallback(
    async (nextFilter = filter, nextPreset = preset) => {
      const generation = ++requestGenerationRef.current;
      const localOnly =
        view === "conversations" ||
        view === "cursor" ||
        view === "cursor-sessions" ||
        view === "worktime" ||
        view === "instructions" ||
        view === "settings";
      if (!localOnly && !hydratedViews.has(view)) {
        apply({ loading: true });
      }
      const commit = (patch: UsageViewPatch) => {
        if (generation === requestGenerationRef.current) {
          apply(patch);
        }
      };
      const epoch = dataEpochRef.current;
      const overviewFresh = isViewFresh(
        loadedStampsRef.current,
        "overview",
        nextFilter,
        nextPreset,
        grain,
        epoch,
      );
      const paint: Array<Promise<void>> = [];
      if (optionsEpochRef.current !== epoch) {
        paint.push(
          invoke<FilterOptions>("get_filter_options").then((options) => {
            commit({ options });
            if (generation === requestGenerationRef.current) {
              optionsEpochRef.current = epoch;
            }
          }),
        );
      }
      if (view !== "conversations" && !overviewFresh) {
        paint.push(
          invoke<OverviewDto>("get_overview", { filter: nextFilter }).then((overview) => {
            commit({ overview });
          }),
        );
      }
      const tasks: Array<Promise<void>> = [];
      if (view === "overview" || view === "trend") {
        tasks.push(
          invoke<SeriesPoint[]>("get_trend", { filter: nextFilter, grain }).then((trend) => {
            commit({ trend });
          }),
        );
      }
      if (view === "overview") {
        const prev = previousFilter(nextFilter, nextPreset);
        const heat = heatmapFilter(nextFilter);
        tasks.push(
          invoke<NamedAmount[]>("get_breakdown", {
            query: { filter: nextFilter, dimension: "model" },
          }).then((models) => {
            commit({ models });
          }),
          invoke<NamedAmount[]>("get_breakdown", {
            query: { filter: nextFilter, dimension: "project" },
          }).then((projects) => {
            commit({ projects });
          }),
          invoke<SessionRow[]>("get_top_sessions", { filter: nextFilter, limit: 8 }).then(
            (sessions) => {
              commit({ sessions });
            },
          ),
          invoke<BillingWindowsDto>("get_billing_windows", { filter: nextFilter }).then(
            (billingWindows) => {
              commit({ billingWindows });
            },
          ),
          invoke<OfficialQuotaDto>("get_official_quota").then((officialQuota) => {
            commit({ officialQuota });
            void invoke<OfficialQuotaDto>("refresh_official_quota")
              .then((refreshed) => {
                commit({ officialQuota: refreshed });
              })
              .catch(() => undefined);
          }),
          invoke<CursorAccountUsageDto>("get_cursor_account_usage", {
            filter: nextFilter,
          }).then((cursorAccountUsage) => {
            commit({ cursorAccountUsage });
          }),
          invoke<BudgetStatusDto>("get_budget_status").then((budgetStatus) => {
            commit({ budgetStatus });
          }),
          invoke<SeriesPoint[]>("get_trend", { filter: heat.filter, grain: "day" }).then(
            (heatmap) => {
              commit({ heatmap, heatmapRange: { from: heat.fromDate, to: heat.toDate } });
            },
          ),
        );
        if (prev) {
          tasks.push(
            invoke<OverviewDto>("get_overview", { filter: prev }).then((previous) => {
              commit({ previous });
            }),
          );
        } else {
          commit({ previous: null });
        }
      }
      if (view === "application") {
        tasks.push(
          invoke<ApplicationAnalyticsDto>("get_application_analytics", {
            filter: nextFilter,
            grain,
          }).then((applicationAnalytics) => {
            commit({ applicationAnalytics });
          }),
        );
      }
      if (view === "model") {
        tasks.push(
          invoke<NamedAmount[]>("get_breakdown", {
            query: { filter: nextFilter, dimension: "model" },
          }).then((models) => {
            commit({ models });
          }),
        );
      }
      if (view === "provider") {
        tasks.push(
          invoke<NamedAmount[]>("get_breakdown", {
            query: { filter: nextFilter, dimension: "provider" },
          }).then((providerBreakdown) => {
            commit({ providerBreakdown });
          }),
        );
      }
      if (view === "project") {
        tasks.push(
          invoke<NamedAmount[]>("get_breakdown", {
            query: { filter: nextFilter, dimension: "project" },
          }).then((projects) => {
            commit({ projects });
          }),
        );
      }
      if (view === "cursor") {
        apply({ codeVolumeLoading: true });
        tasks.push(
          invoke<CodeVolumeSummary>("get_code_volume")
            .then((codeVolume) => {
              commit({ codeVolume });
            })
            .finally(() => {
              if (generation === requestGenerationRef.current) {
                apply({ codeVolumeLoading: false });
              }
            }),
        );
      }
      if (view === "cursor-sessions") {
        apply({ cursorSessionLoading: true });
        tasks.push(
          invoke<CursorSessionSummaryDto>("get_cursor_session_summary")
            .then((cursorSessionSummary) => {
              commit({ cursorSessionSummary });
            })
            .finally(() => {
              if (generation === requestGenerationRef.current) {
                apply({ cursorSessionLoading: false });
              }
            }),
        );
      }
      if (view === "settings") {
        tasks.push(
          invoke<PriceTable>("get_prices").then((prices) => {
            commit({ prices });
          }),
          invoke<SourceDiagnostic[]>("get_source_diagnostics").then((diagnostics) => {
            commit({ diagnostics });
          }),
          invoke<BudgetStatusDto>("get_budget_status").then((budgetStatus) => {
            commit({ budgetStatus });
          }),
          invoke<OfficialQuotaDto>("get_official_quota").then((officialQuota) => {
            commit({ officialQuota });
          }),
        );
      }
      try {
        await Promise.all(paint);
        if (
          generation === requestGenerationRef.current &&
          (view === "overview" || view === "cursor" || view === "cursor-sessions")
        ) {
          apply({ loading: false });
        }
        if (tasks.length > 0) {
          await Promise.all(tasks);
        }
        if (generation === requestGenerationRef.current) {
          markHydrated(view, nextFilter, nextPreset);
        }
      } finally {
        if (generation === requestGenerationRef.current) {
          apply({ loading: false });
        }
      }
      if (generation === requestGenerationRef.current) {
        apply({ updatedAt: new Date().toISOString() });
      }
    },
    [
      view,
      filter,
      preset,
      grain,
      hydratedViews,
      markHydrated,
      requestGenerationRef,
      dataEpochRef,
      loadedStampsRef,
      optionsEpochRef,
      apply,
    ],
  );

  const refreshTrend = useCallback(
    async (nextFilter = filter) => {
      const generation = ++requestGenerationRef.current;
      const commit = (patch: UsageViewPatch) => {
        if (generation === requestGenerationRef.current) {
          apply(patch);
        }
      };
      const tasks: Array<Promise<void>> = [];
      if (view === "overview" || view === "trend") {
        tasks.push(
          invoke<SeriesPoint[]>("get_trend", { filter: nextFilter, grain }).then((trend) => {
            commit({ trend });
          }),
        );
      }
      if (view === "application") {
        tasks.push(
          invoke<ApplicationAnalyticsDto>("get_application_analytics", {
            filter: nextFilter,
            grain,
          }).then((applicationAnalytics) => {
            commit({ applicationAnalytics });
          }),
        );
      }
      if (tasks.length === 0) {
        return;
      }
      apply({ loading: true });
      try {
        await Promise.all(tasks);
      } finally {
        if (generation === requestGenerationRef.current) {
          apply({ loading: false });
          if (view === "overview" || view === "trend") {
            markHydrated("trend", nextFilter, preset);
            loadedStampsRef.current.overview = viewStamp(
              "overview",
              nextFilter,
              preset,
              grain,
              dataEpochRef.current,
            );
          }
          if (view === "application") {
            markHydrated("application", nextFilter, preset);
          }
        }
      }
      if (generation === requestGenerationRef.current) {
        apply({ updatedAt: new Date().toISOString() });
      }
    },
    [
      view,
      filter,
      preset,
      grain,
      markHydrated,
      requestGenerationRef,
      dataEpochRef,
      loadedStampsRef,
      apply,
    ],
  );

  return { refreshViews, refreshTrend };
}
