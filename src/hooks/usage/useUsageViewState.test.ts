import { describe, expect, it } from "vitest";
import type { OverviewDto } from "../../types";
import { heatmapFilter } from "../../lib/calendar";
import { emptyFilter } from "./constants";
import { createInitialUsageViewState, usageViewReducer } from "./useUsageViewState";

const overview: OverviewDto = {
  total_tokens: 10,
  input_tokens: 4,
  output_tokens: 6,
  cache_read_tokens: 0,
  cache_creation_tokens: 0,
  reasoning_tokens: 0,
  session_count: 1,
  cost: null,
  unpriced: false,
  cost_breakdown: { input: null, output: null, cache_read: null, cache_creation: null },
  cost_sources: { native: null, user: null, snapshot: null, unpriced_records: 0 },
};

describe("createInitialUsageViewState", () => {
  it("matches the previous useState defaults", () => {
    const heat = heatmapFilter(emptyFilter);
    const state = createInitialUsageViewState("overview");
    expect(state.options).toEqual({ sources: [], models: [], projects: [], providers: [] });
    expect(state.overview).toBeNull();
    expect(state.billingWindows).toBeNull();
    expect(state.officialQuota).toBeNull();
    expect(state.cursorAccountUsage).toBeNull();
    expect(state.previous).toBeNull();
    expect(state.trend).toEqual([]);
    expect(state.heatmap).toEqual([]);
    expect(state.heatmapRange).toEqual({ from: heat.fromDate, to: heat.toDate });
    expect(state.applicationAnalytics).toBeNull();
    expect(state.models).toEqual([]);
    expect(state.projects).toEqual([]);
    expect(state.providerBreakdown).toEqual([]);
    expect(state.sessions).toEqual([]);
    expect(state.prices).toEqual({ prices: [] });
    expect(state.budgetStatus).toBeNull();
    expect(state.diagnostics).toEqual([]);
    expect(state.codeVolume).toBeNull();
    expect(state.codeVolumeLoading).toBe(false);
    expect(state.cursorSessionSummary).toBeNull();
    expect(state.cursorSessionLoading).toBe(false);
    expect(state.loading).toBe(true);
    expect(state.updatedAt).toBeNull();
  });

  it("turns on the matching local loading flag for the landing view", () => {
    expect(createInitialUsageViewState("cursor").codeVolumeLoading).toBe(true);
    expect(createInitialUsageViewState("cursor-sessions").cursorSessionLoading).toBe(true);
  });
});

describe("usageViewReducer", () => {
  it("shallow-merges a patch and leaves omitted fields unchanged", () => {
    const initial = createInitialUsageViewState("overview");
    const next = usageViewReducer(initial, { overview, loading: false });
    expect(next.overview).toBe(overview);
    expect(next.loading).toBe(false);
    expect(next.trend).toBe(initial.trend);
    expect(next.models).toBe(initial.models);
    expect(next.heatmapRange).toEqual(initial.heatmapRange);
  });
});
