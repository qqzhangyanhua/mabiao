import { beforeEach, describe, expect, it, vi } from "vitest";
import { heatmapFilter } from "../../lib/calendar";
import type { Filter, View } from "../../types";
import { viewStamp, views, viewsWarmedBy } from "../viewCache";
import type { UsageViewPatch } from "./useUsageViewState";
import type { ViewRefreshContext } from "./viewRefresh";

const invokeMock = vi.fn<(...args: unknown[]) => Promise<unknown>>();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const { runTrendRefresh, runViewRefresh } = await import("./viewRefresh");

const rangedFilter: Filter = {
  from: "2026-08-08T00:00:00.000Z",
  to: "2026-08-15T00:00:00.000Z",
  sources: ["claude"],
  models: ["opus"],
  projects: ["/app"],
  providers: ["anthropic"],
};
const previousRange: Filter = {
  ...rangedFilter,
  from: "2026-08-01T00:00:00.000Z",
  to: "2026-08-08T00:00:00.000Z",
};
const heat = heatmapFilter(rangedFilter);

const OPTIONS = {
  sources: ["opt-s"],
  models: ["opt-m"],
  projects: ["opt-p"],
  providers: ["opt-v"],
};
const OVERVIEW = { total_tokens: 101 };
const PREVIOUS = { total_tokens: 202 };
const TREND = [{ bucket: "trend" }];
const HEATMAP = [{ bucket: "heat" }];
const MODELS = [{ name: "opus" }];
const PROJECTS = [{ name: "/app" }];
const PROVIDERS = [{ name: "anthropic" }];
const SESSIONS = [{ id: "sess-1" }];
const BILLING = { now: "billing" };
const QUOTA = { accounts: ["cached"] };
const QUOTA_REFRESHED = { accounts: ["fresh"] };
const CURSOR_ACCOUNT = { total_tokens: 9 };
const BUDGET = { monthly_usd: 20 };
const APPLICATION = { sources: ["claude"] };
const PRICES = { prices: [{ model: "opus" }] };
const DIAGNOSTICS = [{ source: "claude", detected: true }];
const CODE_VOLUME = { added: 11 };
const CURSOR_SESSIONS = { session_count: 3 };

type CommandCall = { command: string; args?: unknown };
type ApplyFn = ReturnType<typeof vi.fn<(patch: UsageViewPatch) => void>>;
type Landed = Record<string, unknown>;
type TestContext = Omit<ViewRefreshContext, "apply" | "markHydrated"> & {
  apply: ApplyFn;
  markHydrated: ReturnType<
    typeof vi.fn<(target: View, nextFilter: Filter, nextPreset: string) => void>
  >;
};
type ViewCommandCase = { view: View; commands: CommandCall[]; landed: Landed };

const LOCAL_VIEWS = [
  "conversations",
  "cursor",
  "cursor-sessions",
  "worktime",
  "instructions",
  "settings",
] as const satisfies readonly View[];

function payloadRecord(payload: unknown): Record<string, unknown> | undefined {
  if (!payload || typeof payload !== "object") {
    return undefined;
  }
  return payload as Record<string, unknown>;
}

function isFilter(value: unknown): value is Filter {
  return Boolean(value && typeof value === "object" && "from" in value && "to" in value);
}

const SIMPLE_RESULTS: Record<string, unknown> = {
  get_filter_options: OPTIONS,
  get_top_sessions: SESSIONS,
  get_billing_windows: BILLING,
  get_official_quota: QUOTA,
  refresh_official_quota: QUOTA_REFRESHED,
  get_cursor_account_usage: CURSOR_ACCOUNT,
  get_budget_status: BUDGET,
  get_application_analytics: APPLICATION,
  get_prices: PRICES,
  get_source_diagnostics: DIAGNOSTICS,
  get_code_volume: CODE_VOLUME,
  get_cursor_session_summary: CURSOR_SESSIONS,
};

function resultFor(command: string, payload: unknown): unknown {
  const record = payloadRecord(payload);
  if (command === "get_overview") {
    const filter = record?.filter;
    if (isFilter(filter) && filter.from === previousRange.from && filter.to === previousRange.to) {
      return PREVIOUS;
    }
    return OVERVIEW;
  }
  if (command === "get_trend") {
    const filter = record?.filter;
    return isFilter(filter) && filter.from === heat.filter.from ? HEATMAP : TREND;
  }
  if (command === "get_breakdown") {
    const dimension = payloadRecord(record?.query)?.dimension;
    if (dimension === "model") {
      return MODELS;
    }
    return dimension === "project" ? PROJECTS : PROVIDERS;
  }
  return SIMPLE_RESULTS[command] ?? {};
}

function makeContext(
  overrides: Partial<Omit<ViewRefreshContext, "apply" | "markHydrated">> = {},
): TestContext {
  return {
    view: "overview",
    grain: "week",
    hydratedViews: new Set<View>(),
    requestGenerationRef: { current: 0 },
    dataEpochRef: { current: 0 },
    loadedStampsRef: { current: {} },
    optionsEpochRef: { current: -1 },
    markHydrated: vi.fn(),
    apply: vi.fn<(patch: UsageViewPatch) => void>(),
    nextFilter: rangedFilter,
    nextPreset: "7",
    ...overrides,
  };
}

function recordedCalls(): CommandCall[] {
  return invokeMock.mock.calls.map((call) => {
    const command = String(call[0]);
    return call.length < 2 ? { command } : { command, args: call[1] };
  });
}

function serializeCall(call: CommandCall): string {
  return JSON.stringify([call.command, call.args ?? null]);
}

function expectCommandSet(expected: CommandCall[]) {
  const byKey = (left: CommandCall, right: CommandCall) =>
    serializeCall(left).localeCompare(serializeCall(right));
  expect([...recordedCalls()].sort(byKey)).toEqual([...expected].sort(byKey));
}

function dataPatches(apply: ApplyFn): Landed {
  const merged: Landed = {};
  for (const [patch] of apply.mock.calls) {
    const data: Landed = { ...patch };
    delete data.loading;
    delete data.updatedAt;
    delete data.codeVolumeLoading;
    delete data.cursorSessionLoading;
    Object.assign(merged, data);
  }
  return merged;
}

function loadingFlags(apply: ApplyFn): boolean[] {
  return apply.mock.calls
    .map(([patch]) => patch.loading)
    .filter((value): value is boolean => typeof value === "boolean");
}

function cmd(command: string, args?: unknown): CommandCall {
  return args === undefined ? { command } : { command, args };
}

function breakdown(dimension: string): CommandCall {
  return cmd("get_breakdown", { query: { filter: rangedFilter, dimension } });
}

const filterOptions = [cmd("get_filter_options")];

const VIEW_CASES: ViewCommandCase[] = [
  {
    view: "overview",
    commands: [
      cmd("get_filter_options"),
      cmd("get_overview", { filter: rangedFilter }),
      cmd("get_trend", { filter: rangedFilter, grain: "week" }),
      breakdown("model"),
      breakdown("project"),
      cmd("get_top_sessions", { filter: rangedFilter, limit: 8 }),
      cmd("get_billing_windows", { filter: rangedFilter }),
      cmd("get_official_quota"),
      cmd("refresh_official_quota"),
      cmd("get_cursor_account_usage", { filter: rangedFilter }),
      cmd("get_budget_status"),
      cmd("get_trend", { filter: heat.filter, grain: "day" }),
      cmd("get_overview", { filter: previousRange }),
    ],
    landed: {
      options: OPTIONS,
      overview: OVERVIEW,
      trend: TREND,
      models: MODELS,
      projects: PROJECTS,
      sessions: SESSIONS,
      billingWindows: BILLING,
      officialQuota: QUOTA_REFRESHED,
      cursorAccountUsage: CURSOR_ACCOUNT,
      budgetStatus: BUDGET,
      heatmap: HEATMAP,
      heatmapRange: { from: heat.fromDate, to: heat.toDate },
      previous: PREVIOUS,
    },
  },
  {
    view: "trend",
    commands: [...filterOptions, cmd("get_trend", { filter: rangedFilter, grain: "week" })],
    landed: { options: OPTIONS, trend: TREND },
  },
  {
    view: "application",
    commands: [
      ...filterOptions,
      cmd("get_application_analytics", { filter: rangedFilter, grain: "week" }),
    ],
    landed: { options: OPTIONS, applicationAnalytics: APPLICATION },
  },
  {
    view: "model",
    commands: [...filterOptions, breakdown("model")],
    landed: { options: OPTIONS, models: MODELS },
  },
  {
    view: "provider",
    commands: [...filterOptions, breakdown("provider")],
    landed: { options: OPTIONS, providerBreakdown: PROVIDERS },
  },
  {
    view: "project",
    commands: [...filterOptions, breakdown("project")],
    landed: { options: OPTIONS, projects: PROJECTS },
  },
  { view: "conversations", commands: filterOptions, landed: { options: OPTIONS } },
  {
    view: "cursor",
    commands: [...filterOptions, cmd("get_code_volume")],
    landed: { options: OPTIONS, codeVolume: CODE_VOLUME },
  },
  {
    view: "cursor-sessions",
    commands: [...filterOptions, cmd("get_cursor_session_summary")],
    landed: { options: OPTIONS, cursorSessionSummary: CURSOR_SESSIONS },
  },
  { view: "worktime", commands: filterOptions, landed: { options: OPTIONS } },
  { view: "instructions", commands: filterOptions, landed: { options: OPTIONS } },
  {
    view: "settings",
    commands: [
      ...filterOptions,
      cmd("get_prices"),
      cmd("get_source_diagnostics"),
      cmd("get_budget_status"),
      cmd("get_official_quota"),
    ],
    landed: {
      options: OPTIONS,
      prices: PRICES,
      diagnostics: DIAGNOSTICS,
      budgetStatus: BUDGET,
      officialQuota: QUOTA,
    },
  },
];

function stubInvoke() {
  invokeMock.mockReset();
  invokeMock.mockImplementation((command, payload) =>
    Promise.resolve(resultFor(String(command), payload)),
  );
}

async function expectIndependentLoading(
  view: View,
  flag: "codeVolumeLoading" | "cursorSessionLoading",
  command: string,
) {
  const events: string[] = [];
  const ctx = makeContext({ view });
  ctx.apply.mockImplementation((patch) => {
    if (patch[flag] === true) {
      events.push("on");
    }
    if (patch[flag] === false) {
      events.push("off");
    }
  });
  invokeMock.mockImplementation((name, payload) => {
    events.push(`invoke:${String(name)}`);
    return Promise.resolve(resultFor(String(name), payload));
  });
  await runViewRefresh(ctx);
  const invokeAt = events.indexOf(`invoke:${command}`);
  expect(events.indexOf("on")).toBeGreaterThanOrEqual(0);
  expect(events.indexOf("on")).toBeLessThan(invokeAt);
  expect(invokeAt).toBeLessThan(events.lastIndexOf("off"));
}

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function gateCommands(names: string[]) {
  const pending = new Map<string, Deferred<unknown>[]>();
  invokeMock.mockImplementation((command, payload) => {
    const key = String(command);
    if (!names.includes(key)) {
      return Promise.resolve(resultFor(key, payload));
    }
    const d = deferred<unknown>();
    const list = pending.get(key) ?? [];
    list.push(d);
    pending.set(key, list);
    return d.promise;
  });
  return {
    take(command: string): Deferred<unknown> {
      const d = pending.get(command)?.shift();
      if (!d) {
        throw new Error(`no pending ${command}`);
      }
      return d;
    },
  };
}

async function microtasks() {
  await Promise.resolve();
  await Promise.resolve();
}

function hasUpdatedAt(apply: ApplyFn): boolean {
  return apply.mock.calls.some(([patch]) => typeof patch.updatedAt === "string");
}

describe("runViewRefresh", () => {
  beforeEach(stubInvoke);

  describe("command sets", () => {
    it("covers every registered view exactly once", () => {
      expect(VIEW_CASES.map((item) => item.view).sort()).toEqual([...views].sort());
    });

    it.each(VIEW_CASES)(
      "sends the command set for the $view view",
      async ({ view, commands, landed }) => {
        const ctx = makeContext({ view });
        await runViewRefresh(ctx);
        expectCommandSet(commands);
        expect(dataPatches(ctx.apply)).toEqual(landed);
      },
    );
  });

  describe("skip conditions", () => {
    it("does not fetch overview on views that do not render it", async () => {
      for (const view of views.filter((item) => item !== "overview")) {
        invokeMock.mockClear();
        await runViewRefresh(makeContext({ view }));
        expect(
          recordedCalls().some((call) => call.command === "get_overview"),
          `${view} should not query get_overview`,
        ).toBe(false);
      }
    });

    it("skips the current overview query when the overview stamp is fresh", async () => {
      const epoch = 4;
      const ctx = makeContext({
        view: "overview",
        dataEpochRef: { current: epoch },
        loadedStampsRef: {
          current: { overview: viewStamp("overview", rangedFilter, "7", "week", epoch) },
        },
      });
      await runViewRefresh(ctx);
      expect(recordedCalls().filter((call) => call.command === "get_overview")).toEqual([
        cmd("get_overview", { filter: previousRange }),
      ]);
    });

    it("skips filter options when the options epoch matches the data epoch", async () => {
      const ctx = makeContext({
        view: "conversations",
        dataEpochRef: { current: 3 },
        optionsEpochRef: { current: 3 },
      });
      await runViewRefresh(ctx);
      expectCommandSet([]);
      expect(ctx.optionsEpochRef.current).toBe(3);
    });

    it("fetches filter options when epochs differ and advances the options epoch", async () => {
      const ctx = makeContext({
        view: "conversations",
        dataEpochRef: { current: 3 },
        optionsEpochRef: { current: -1 },
      });
      await runViewRefresh(ctx);
      expectCommandSet([cmd("get_filter_options")]);
      expect(dataPatches(ctx.apply)).toEqual({ options: OPTIONS });
      expect(ctx.optionsEpochRef.current).toBe(3);
    });

    it.each(LOCAL_VIEWS)("does not turn on loading for the %s view", async (view) => {
      const ctx = makeContext({ view });
      await runViewRefresh(ctx);
      expect(loadingFlags(ctx.apply)).not.toContain(true);
    });

    it("turns on code volume loading before the request and turns it off after", async () => {
      await expectIndependentLoading("cursor", "codeVolumeLoading", "get_code_volume");
    });

    it("turns on cursor session loading before the request and turns it off after", async () => {
      await expectIndependentLoading(
        "cursor-sessions",
        "cursorSessionLoading",
        "get_cursor_session_summary",
      );
    });

    it("clears the previous overview when the preset has no previous window", async () => {
      const ctx = makeContext({ view: "overview", nextPreset: "all" });
      await runViewRefresh(ctx);
      expect(recordedCalls().filter((call) => call.command === "get_overview")).toEqual([
        cmd("get_overview", { filter: rangedFilter }),
      ]);
      expect(dataPatches(ctx.apply).previous).toBeNull();
    });

    it("keeps the heatmap query on a daily grain and its own range when the page grain is week", async () => {
      await runViewRefresh(makeContext({ view: "overview", grain: "week" }));
      const heatmapCall = recordedCalls().find(
        (call) => call.command === "get_trend" && payloadRecord(call.args)?.grain === "day",
      );
      const pageTrend = recordedCalls().find(
        (call) => call.command === "get_trend" && payloadRecord(call.args)?.grain === "week",
      );
      expect(heatmapCall?.args).toEqual({ filter: heat.filter, grain: "day" });
      expect(pageTrend?.args).toEqual({ filter: rangedFilter, grain: "week" });
      expect(heat.filter.from).not.toBe(rangedFilter.from);
      expect(heat.filter.to).not.toBe(rangedFilter.to);
    });

    it("appends official quota refresh after the cached quota query returns", async () => {
      const events: string[] = [];
      invokeMock.mockImplementation((command, payload) => {
        events.push(`start:${String(command)}`);
        return Promise.resolve(resultFor(String(command), payload)).then((value) => {
          events.push(`done:${String(command)}`);
          return value;
        });
      });
      await runViewRefresh(makeContext({ view: "overview" }));
      expect(events.indexOf("done:get_official_quota")).toBeGreaterThanOrEqual(0);
      expect(events.indexOf("done:get_official_quota")).toBeLessThan(
        events.indexOf("start:refresh_official_quota"),
      );
    });

    it("still lands the rest of the overview when official quota refresh fails", async () => {
      invokeMock.mockImplementation((command, payload) => {
        if (command === "refresh_official_quota") {
          return Promise.reject(new Error("network"));
        }
        return Promise.resolve(resultFor(String(command), payload));
      });
      const ctx = makeContext({ view: "overview" });
      await expect(runViewRefresh(ctx)).resolves.toBeUndefined();
      expect(recordedCalls().some((call) => call.command === "refresh_official_quota")).toBe(true);
      const data = dataPatches(ctx.apply);
      expect(data.officialQuota).toBe(QUOTA);
      expect(data.trend).toBe(TREND);
      expect(data.models).toBe(MODELS);
      expect(data.projects).toBe(PROJECTS);
      expect(data.sessions).toBe(SESSIONS);
      expect(data.overview).toBe(OVERVIEW);
    });

    it("loads every dataset that viewCache marks warm after overview", async () => {
      expect(viewsWarmedBy("overview")).toEqual(["overview", "trend", "model", "project"]);
      const ctx = makeContext({ view: "overview" });
      await runViewRefresh(ctx);
      const data = dataPatches(ctx.apply);
      expect(data.overview).toBe(OVERVIEW);
      expect(data.trend).toBe(TREND);
      expect(data.models).toBe(MODELS);
      expect(data.projects).toBe(PROJECTS);
    });
  });

  describe("generation", () => {
    const matchedEpochs = { optionsEpochRef: { current: 0 }, dataEpochRef: { current: 0 } };

    it("drops command results from a stale generation", async () => {
      const gate = gateCommands(["get_trend"]);
      const ctx = makeContext({ view: "trend", ...matchedEpochs });
      const pending = runViewRefresh(ctx);
      const callsAtStart = ctx.apply.mock.calls.length;
      ctx.requestGenerationRef.current += 1;
      gate.take("get_trend").resolve(TREND);
      await pending;
      expect(ctx.apply.mock.calls.length).toBe(callsAtStart);
      expect(dataPatches(ctx.apply).trend).toBeUndefined();
    });

    it("does not clear loading from a stale generation", async () => {
      const gate = gateCommands(["get_trend"]);
      const ctx = makeContext({ view: "trend", ...matchedEpochs });
      const pending = runViewRefresh(ctx);
      expect(loadingFlags(ctx.apply)).toEqual([true]);
      ctx.requestGenerationRef.current += 1;
      gate.take("get_trend").resolve(TREND);
      await pending;
      expect(loadingFlags(ctx.apply)).toEqual([true]);
    });

    it("does not mark the view hydrated from a stale generation", async () => {
      const gate = gateCommands(["get_trend"]);
      const ctx = makeContext({ view: "trend", ...matchedEpochs });
      const pending = runViewRefresh(ctx);
      ctx.requestGenerationRef.current += 1;
      gate.take("get_trend").resolve(TREND);
      await pending;
      expect(ctx.markHydrated).not.toHaveBeenCalled();
    });

    it("does not advance the options epoch from a stale generation", async () => {
      const gate = gateCommands(["get_filter_options"]);
      const ctx = makeContext({
        view: "conversations",
        dataEpochRef: { current: 4 },
        optionsEpochRef: { current: -1 },
      });
      const pending = runViewRefresh(ctx);
      ctx.requestGenerationRef.current += 1;
      gate.take("get_filter_options").resolve(OPTIONS);
      await pending;
      expect(ctx.optionsEpochRef.current).toBe(-1);
      expect(dataPatches(ctx.apply).options).toBeUndefined();
    });

    it("lands the latest generation and clears loading, hydrates, and writes updatedAt", async () => {
      const staleTrend = [{ bucket: "stale" }];
      const freshTrend = [{ bucket: "fresh" }];
      const gate = gateCommands(["get_trend"]);
      const ctx = makeContext({ view: "trend", ...matchedEpochs });
      const first = runViewRefresh(ctx);
      const second = runViewRefresh(ctx);
      const stale = gate.take("get_trend");
      const fresh = gate.take("get_trend");
      fresh.resolve(freshTrend);
      await second;
      expect(dataPatches(ctx.apply).trend).toBe(freshTrend);
      stale.resolve(staleTrend);
      await first;
      expect(dataPatches(ctx.apply).trend).toBe(freshTrend);
      expect(ctx.markHydrated).toHaveBeenCalledTimes(1);
      expect(ctx.markHydrated).toHaveBeenCalledWith("trend", rangedFilter, "7");
      expect(loadingFlags(ctx.apply).at(-1)).toBe(false);
      expect(hasUpdatedAt(ctx.apply)).toBe(true);
    });

    it.each(["overview", "cursor", "cursor-sessions"] as const)(
      "turns off loading after the first stage on %s while later results still land",
      async (view) => {
        const gated =
          view === "overview"
            ? ["get_trend"]
            : view === "cursor"
              ? ["get_code_volume"]
              : ["get_cursor_session_summary"];
        const gate = gateCommands(gated);
        const ctx = makeContext({ view });
        const pending = runViewRefresh(ctx);
        await microtasks();
        expect(loadingFlags(ctx.apply)).toContain(false);
        const data = dataPatches(ctx.apply);
        if (view === "overview") {
          expect(data.trend).toBeUndefined();
          gate.take("get_trend").resolve(TREND);
          gate.take("get_trend").resolve(HEATMAP);
        } else if (view === "cursor") {
          expect(data.codeVolume).toBeUndefined();
          gate.take("get_code_volume").resolve(CODE_VOLUME);
        } else {
          expect(data.cursorSessionSummary).toBeUndefined();
          gate.take("get_cursor_session_summary").resolve(CURSOR_SESSIONS);
        }
        await pending;
        const landed = dataPatches(ctx.apply);
        if (view === "overview") {
          expect(landed.trend).toBe(TREND);
        } else if (view === "cursor") {
          expect(landed.codeVolume).toBe(CODE_VOLUME);
        } else {
          expect(landed.cursorSessionSummary).toBe(CURSOR_SESSIONS);
        }
      },
    );

    it("clears loading and rethrows when a command fails", async () => {
      invokeMock.mockImplementation((command, payload) => {
        if (command === "get_trend") {
          return Promise.reject(new Error("boom"));
        }
        return Promise.resolve(resultFor(String(command), payload));
      });
      const ctx = makeContext({ view: "trend", ...matchedEpochs });
      await expect(runViewRefresh(ctx)).rejects.toThrow("boom");
      expect(loadingFlags(ctx.apply)).toContain(false);
      expect(ctx.markHydrated).not.toHaveBeenCalled();
    });

    it("lets only the latest generation clear code volume loading", async () => {
      const gate = gateCommands(["get_code_volume"]);
      const ctx = makeContext({ view: "cursor", ...matchedEpochs });
      const first = runViewRefresh(ctx);
      const second = runViewRefresh(ctx);
      const stale = gate.take("get_code_volume");
      const fresh = gate.take("get_code_volume");
      stale.resolve(CODE_VOLUME);
      await first;
      expect(ctx.apply.mock.calls.some(([patch]) => patch.codeVolumeLoading === false)).toBe(false);
      fresh.resolve(CODE_VOLUME);
      await second;
      expect(ctx.apply.mock.calls.some(([patch]) => patch.codeVolumeLoading === false)).toBe(true);
    });

    it("lets only the latest generation clear cursor session loading", async () => {
      const gate = gateCommands(["get_cursor_session_summary"]);
      const ctx = makeContext({ view: "cursor-sessions", ...matchedEpochs });
      const first = runViewRefresh(ctx);
      const second = runViewRefresh(ctx);
      const stale = gate.take("get_cursor_session_summary");
      const fresh = gate.take("get_cursor_session_summary");
      stale.resolve(CURSOR_SESSIONS);
      await first;
      expect(ctx.apply.mock.calls.some(([patch]) => patch.cursorSessionLoading === false)).toBe(
        false,
      );
      fresh.resolve(CURSOR_SESSIONS);
      await second;
      expect(ctx.apply.mock.calls.some(([patch]) => patch.cursorSessionLoading === false)).toBe(
        true,
      );
    });
  });
});

describe("runTrendRefresh", () => {
  beforeEach(stubInvoke);

  it("does not increment generation when there is nothing to fetch", async () => {
    const ctx = makeContext({ view: "model", requestGenerationRef: { current: 7 } });
    await runTrendRefresh(ctx);
    expect(ctx.requestGenerationRef.current).toBe(7);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("does not cancel an in-flight view refresh when there is nothing to fetch", async () => {
    const gate = gateCommands(["get_trend"]);
    const ctx = makeContext({
      view: "trend",
      optionsEpochRef: { current: 0 },
      dataEpochRef: { current: 0 },
    });
    const pending = runViewRefresh(ctx);
    await runTrendRefresh({ ...ctx, view: "model" });
    expect(ctx.requestGenerationRef.current).toBe(1);
    gate.take("get_trend").resolve(TREND);
    await pending;
    expect(dataPatches(ctx.apply).trend).toBe(TREND);
  });

  it("does not write the overview stamp from a stale generation", async () => {
    const gate = gateCommands(["get_trend"]);
    const ctx = makeContext({ view: "overview" });
    const pending = runTrendRefresh(ctx);
    ctx.requestGenerationRef.current += 1;
    gate.take("get_trend").resolve(TREND);
    await pending;
    expect(ctx.loadedStampsRef.current.overview).toBeUndefined();
    expect(ctx.markHydrated).not.toHaveBeenCalled();
    expect(dataPatches(ctx.apply).trend).toBeUndefined();
  });
});
