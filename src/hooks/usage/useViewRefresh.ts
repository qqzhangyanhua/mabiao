import { useCallback, useRef, type MutableRefObject } from "react";
import type { Filter, Grain, View } from "../../types";
import type { UsageViewPatch } from "./useUsageViewState";
import { runTrendRefresh, runViewRefresh } from "./viewRefresh";

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
  const wideRefreshGenerationRef = useRef<number | null>(null);

  const refreshViews = useCallback(
    async (nextFilter = filter, nextPreset = preset) =>
      runViewRefresh({
        view,
        grain,
        hydratedViews,
        requestGenerationRef,
        dataEpochRef,
        loadedStampsRef,
        optionsEpochRef,
        wideRefreshGenerationRef,
        markHydrated,
        apply,
        nextFilter,
        nextPreset,
      }),
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
      wideRefreshGenerationRef,
      apply,
    ],
  );

  const refreshTrend = useCallback(
    async (nextFilter = filter) =>
      runTrendRefresh({
        view,
        grain,
        hydratedViews,
        requestGenerationRef,
        dataEpochRef,
        loadedStampsRef,
        optionsEpochRef,
        wideRefreshGenerationRef,
        markHydrated,
        apply,
        nextFilter,
        nextPreset: preset,
      }),
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
      wideRefreshGenerationRef,
      apply,
    ],
  );

  return { refreshViews, refreshTrend };
}
