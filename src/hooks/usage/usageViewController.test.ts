import { describe, expect, it } from "vitest";
import { StatefulUsageBackendFake } from "./usageBackendFake";
import {
  emptyViewScope,
  filtersEqual,
  initialViewScopes,
  syncSharedFilters,
  UsageViewController,
} from "./usageViewController";

function makeController(
  backend = new StatefulUsageBackendFake(),
  initialView: "overview" | "settings" | "trend" | "model" | "project" | "provider" | "application" | "cursor" | "cursor-sessions" | "conversations" = "overview",
) {
  return new UsageViewController({
    backend,
    initialView,
    initialPreset: "all",
  });
}

describe("UsageViewController — overview bundle, warming, and loading", () => {
  it("loads overview via coarse getOverviewBundle while official quota refreshes non-blockingly", async () => {
    const backend = new StatefulUsageBackendFake();
    const quotaRefreshGate = backend.gateNext("refreshOfficialQuota");
    backend.refreshedOfficialQuota = {
      ...backend.state.officialQuota,
      rows: [
        {
          ...backend.state.officialQuota.rows[0],
          plan: "Max",
        },
      ],
    };

    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();

    const snap = controller.getSnapshot();
    expect(backend.count("getOverviewBundle")).toBe(1);
    expect(backend.count("getTrend")).toBe(0);
    expect(backend.count("getBreakdown")).toBe(0);
    expect(backend.count("getBudgetStatus")).toBe(0);
    expect(snap.loading).toBe(false);
    expect(snap.viewHasData).toBe(true);
    expect(snap.overview?.total_tokens).toBe(1_500_000);
    expect(snap.overview?.cost).toBeCloseTo(2.5, 6);
    expect(snap.models.map((m) => m.name)).toEqual(["claude-opus-4", "gpt-5"]);
    expect(snap.projects.some((p) => p.name === "Cursor（账号用量）")).toBe(true);
    expect(snap.budgetStatus?.monthly_budget).toBe(100);
    expect(snap.officialQuota?.rows[0]?.plan).toBe("Pro");
    expect(snap.options.sources).toEqual(["claude", "codex"]);

    // Non-blocking official quota refresh lands after refreshActiveView already resolved
    quotaRefreshGate.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(controller.getSnapshot().officialQuota?.rows[0]?.plan).toBe("Max");
  });

  it("lands overview cleanly even when non-blocking official quota refresh fails", async () => {
    const backend = new StatefulUsageBackendFake();
    backend.failMethod.refreshOfficialQuota = new Error("offline");

    const controller = makeController(backend, "overview");
    await expect(controller.refreshActiveView()).resolves.toBeUndefined();

    const snap = controller.getSnapshot();
    expect(snap.overview?.total_tokens).toBe(1_500_000);
    expect(snap.officialQuota?.rows[0]?.plan).toBe("Pro");
    expect(snap.loading).toBe(false);
  });

  it("warms trend, model, and project views from overview without re-fetching on navigation", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();
    backend.resetCounts();

    await controller.navigate("trend");
    await controller.navigate("model");
    await controller.navigate("project");
    expect(backend.count("getTrend")).toBe(0);
    expect(backend.count("getBreakdown")).toBe(0);
    expect(controller.getSnapshot().breakdown.map((p) => p.name)).toContain("/app");

    // Provider was not warmed by overview, so navigating to it fetches provider breakdown only
    await controller.navigate("provider");
    expect(backend.count("getBreakdown")).toBe(1);
    expect(controller.getSnapshot().breakdown.map((p) => p.name)).toEqual(["anthropic", "openai"]);
  });

  it("uses independent loading flags for cursor and cursor-sessions and never enables global loading on local views", async () => {
    const backend = new StatefulUsageBackendFake();
    const codeGate = backend.gateNext("getCodeVolume");
    const controller = makeController(backend, "cursor");
    const seenLoading: boolean[] = [];
    const seenCodeLoading: boolean[] = [];
    controller.subscribe(() => {
      const s = controller.getSnapshot();
      seenLoading.push(s.loading);
      seenCodeLoading.push(s.codeVolumeLoading);
    });

    const pending = controller.refreshActiveView();
    expect(controller.getSnapshot().codeVolumeLoading).toBe(true);
    codeGate.resolve();
    await pending;

    expect(seenLoading).not.toContain(true);
    expect(controller.getSnapshot().codeVolumeLoading).toBe(false);
    expect(controller.getSnapshot().codeVolume?.total_cost).toBeCloseTo(2.5, 6);
  });
});

describe("UsageViewController — pricing, LiteLLM snapshot, and budget invalidation", () => {
  it("invalidates pricing-dependent views on savePrices and re-queries SQLite without disk ingest", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();
    expect(controller.getSnapshot().overview?.cost).toBeCloseTo(2.5, 6);

    await controller.navigate("provider");
    expect(controller.getSnapshot().breakdown.find((r) => r.name === "anthropic")?.cost).toBeCloseTo(
      2.0,
      6,
    );

    await controller.navigate("settings");
    expect(controller.getSnapshot().budgetStatus?.month_to_date_cost).toBeCloseTo(2.5, 6);
    const revBefore = controller.getSnapshot().pricingRevision;
    backend.resetCounts();

    await controller.savePrices({
      prices: [
        {
          model: "claude-opus-4",
          provider: null,
          input: 10 / 1_000_000,
          output: 0,
          cache_read: 0,
          cache_creation: 0,
          origin: "user",
        },
      ],
    });

    expect(backend.count("ingest")).toBe(0);
    expect(backend.count("getFilterOptions")).toBe(0);
    expect(controller.getSnapshot().pricingRevision).toBe(revBefore + 1);
    expect(controller.getSnapshot().status).toBe("单价已保存");
    expect(controller.getSnapshot().budgetStatus?.month_to_date_cost).toBeCloseTo(10.5, 6);

    // Navigating back to overview, model, project, provider, cursor immediately reflects updated costs
    await controller.navigate("overview");
    expect(controller.getSnapshot().overview?.cost).toBeCloseTo(10.5, 6);
    expect(controller.getSnapshot().models.find((m) => m.name === "claude-opus-4")?.cost).toBeCloseTo(
      10.0,
      6,
    );

    await controller.navigate("provider");
    expect(controller.getSnapshot().breakdown.find((r) => r.name === "anthropic")?.cost).toBeCloseTo(
      10.0,
      6,
    );

    await controller.navigate("cursor");
    expect(controller.getSnapshot().codeVolume?.total_cost).toBeCloseTo(10.5, 6);
    expect(backend.count("ingest")).toBe(0);
  });

  it("refreshes costs from SQLite after LiteLLM snapshot change without triggering disk ingest", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();
    expect(controller.getSnapshot().overview?.cost).toBeCloseTo(2.5, 6);

    await controller.navigate("settings");
    const revBefore = controller.getSnapshot().pricingRevision;
    backend.resetCounts();

    // Simulate LiteLLM snapshot updating gpt-5 fallback price from $1/M to $6/M
    backend.applySnapshotPrices([
      {
        model: "gpt-5",
        provider: null,
        input: 6 / 1_000_000,
        output: 0,
        cache_read: 0,
        cache_creation: 0,
        origin: "snapshot",
      },
    ]);
    await controller.invalidatePricing();

    expect(backend.count("ingest")).toBe(0);
    expect(controller.getSnapshot().pricingRevision).toBe(revBefore + 1);
    expect(controller.getSnapshot().budgetStatus?.month_to_date_cost).toBeCloseTo(5.0, 6);

    await controller.navigate("overview");
    expect(controller.getSnapshot().overview?.cost).toBeCloseTo(5.0, 6);
    expect(controller.getSnapshot().models.find((m) => m.name === "gpt-5")?.cost).toBeCloseTo(3.0, 6);
    expect(backend.count("ingest")).toBe(0);
  });

  it("updates budgetStatus and invalidates overview on saveBudget without disk ingest", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();
    expect(controller.getSnapshot().budgetStatus?.monthly_budget).toBe(100);

    await controller.navigate("settings");
    backend.resetCounts();

    await controller.saveBudget({ monthly_usd: 25 });
    expect(backend.count("ingest")).toBe(0);
    expect(controller.getSnapshot().savingBudget).toBe(false);
    expect(controller.getSnapshot().budgetStatus?.monthly_budget).toBe(25);
    expect(controller.getSnapshot().budgetStatus?.percent_used).toBeCloseTo(10, 6);
    expect(controller.getSnapshot().status).toBe("预算设置已保存");

    await controller.navigate("overview");
    expect(backend.count("getOverviewBundle")).toBe(1);
    expect(controller.getSnapshot().budgetStatus?.monthly_budget).toBe(25);
  });
});

describe("UsageViewController — dedicated backup restore reload", () => {
  it("reloads restored SQLite data, options, prices, budget, diagnostics, and official quota without disk ingest", async () => {
    const backend = new StatefulUsageBackendFake();
    let clearedDetailCache = 0;
    const controller = new UsageViewController({
      backend,
      initialView: "overview",
      initialPreset: "all",
      onClearSessionDetailCache: () => {
        clearedDetailCache += 1;
      },
    });
    await controller.refreshActiveView();
    await controller.navigate("settings");
    const before = controller.getSnapshot();
    backend.resetCounts();

    backend.applyBackupSnapshot({
      records: [
        {
          occurred_at: "2026-07-01T08:00:00.000Z",
          source: "gemini",
          model: "gemini-2.5-pro",
          provider: "google",
          project: "/restored-repo",
          session_id: "restored-1",
          tokens: 400_000,
        },
      ],
      userPrices: [
        {
          model: "gemini-2.5-pro",
          provider: null,
          input: 5 / 1_000_000,
          output: 0,
          cache_read: 0,
          cache_creation: 0,
          origin: "user",
        },
      ],
      snapshotPrices: [],
      budget: { monthly_usd: 42 },
      cursorEvents: [],
      cursorAsOf: null,
      diagnostics: [
        {
          source: "gemini",
          application: "Gemini CLI",
          detected: true,
          root_path: "/restored/.gemini/tmp",
          cached_files: 2,
          record_count: 1,
          total_tokens: 400_000,
          coverage: "2026-07-01",
          archived_record_count: 0,
        },
      ],
      officialQuota: {
        rows: [
          {
            provider: "codex",
            application: "Codex CLI",
            windows: [],
            freshness: "official",
            captured_at: "2026-07-01T09:00:00.000Z",
            error: null,
            todo: null,
            plan: "Plus",
          },
        ],
        alerts_enabled: false,
        stale_after_minutes: 30,
        undetected: [],
        hidden_providers: ["claude"],
      },
      conversationIssues: [{ source: "gemini", path: "/restored/bad.json", message: "corrupt" }],
    });

    await controller.reloadAfterRestore();

    const after = controller.getSnapshot();
    expect(backend.count("ingest")).toBe(0);
    expect(clearedDetailCache).toBe(1);
    expect(after.restoreRevision).toBe(before.restoreRevision + 1);
    expect(after.sessionsRevision).toBe(before.sessionsRevision + 1);
    expect(after.pricingRevision).toBe(before.pricingRevision + 1);
    expect(after.cursorAccountRevision).toBe(before.cursorAccountRevision + 1);
    expect(after.options.sources).toEqual(["gemini"]);
    expect(after.prices.prices[0]?.model).toBe("gemini-2.5-pro");
    expect(after.budgetStatus?.monthly_budget).toBe(42);
    expect(after.budgetStatus?.month_to_date_cost).toBeCloseTo(2.0, 6);
    expect(after.diagnostics[0]?.source).toBe("gemini");
    expect(after.officialQuota?.hidden_providers).toEqual(["claude"]);
    expect(after.conversationIssues).toHaveLength(1);
    expect(after.cursorAccountUsage?.event_count).toBe(0);

    // Navigating back to overview loads restored SQLite usage data (no stale pre-restore cache)
    await controller.navigate("overview");
    expect(controller.getSnapshot().overview?.total_tokens).toBe(400_000);
    expect(controller.getSnapshot().overview?.cost).toBeCloseTo(2.0, 6);
    expect(controller.getSnapshot().projects.map((p) => p.name)).toEqual(["/restored-repo"]);
    expect(backend.count("ingest")).toBe(0);
  });
});

describe("UsageViewController — Cursor Account clear and generation-guarded refresh", () => {
  it("evicts Cursor account ghosts across overview, trend, application, project, and cursor on clear", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();
    expect(controller.getSnapshot().cursorAccountUsage?.event_count).toBe(1);
    expect(controller.getSnapshot().billingWindows?.weekly.some((w) => w.source === "cursor")).toBe(
      true,
    );
    expect(controller.getSnapshot().projects.some((p) => p.name === "Cursor（账号用量）")).toBe(true);

    await controller.navigate("application");
    expect(
      controller.getSnapshot().applicationAnalytics?.by_application.some((r) => r.source === "cursor"),
    ).toBe(true);

    await controller.navigate("settings");
    const revBefore = controller.getSnapshot().cursorAccountRevision;
    backend.resetCounts();

    await controller.clearCursorAccountUsage();

    const rightAfterClear = controller.getSnapshot();
    expect(rightAfterClear.cursorAccountRevision).toBe(revBefore + 1);
    expect(rightAfterClear.cursorAccountUsage?.event_count).toBe(0);
    expect(rightAfterClear.billingWindows).toBeNull();
    expect(rightAfterClear.trend).toEqual([]);
    expect(rightAfterClear.applicationAnalytics).toBeNull();
    expect(rightAfterClear.projects).toEqual([]);
    expect(backend.count("ingest")).toBe(0);

    await controller.navigate("overview");
    expect(controller.getSnapshot().cursorAccountUsage?.event_count).toBe(0);
    expect(controller.getSnapshot().billingWindows?.weekly.some((w) => w.source === "cursor")).toBe(
      false,
    );
    expect(controller.getSnapshot().projects.some((p) => p.name === "Cursor（账号用量）")).toBe(
      false,
    );

    await controller.navigate("application");
    expect(
      controller.getSnapshot().applicationAnalytics?.by_application.some((r) => r.source === "cursor"),
    ).toBe(false);
  });

  it("reloads using the newer filter when a slow background Cursor refresh completes mid-filter-change", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();

    // Start slow background Cursor refresh (e.g. 10-min auto-refresh)
    const refreshGate = backend.gateNext("refreshCursorAccountUsage");
    backend.refreshedCursorEvents = [
      { occurred_at: "2026-08-16T15:00:00.000Z", model: "claude-opus-4", tokens: 900_000 },
    ];
    const backgroundRefresh = controller.refreshCursorAccount();

    // User narrows filter to model = claude-opus-4 while Cursor network pull is in flight
    await controller.applyFilter({
      ...controller.getSnapshot().filter,
      models: ["claude-opus-4"],
    });
    expect(controller.getSnapshot().overview?.total_tokens).toBe(1_000_000);
    expect(controller.getSnapshot().cursorAccountUsage?.total_tokens).toBe(0);

    // Background Cursor refresh finishes; overview reloads WITH models=["claude-opus-4"] intact
    refreshGate.resolve();
    await backgroundRefresh;

    const snap = controller.getSnapshot();
    expect(snap.filter.models).toEqual(["claude-opus-4"]);
    expect(snap.overview?.total_tokens).toBe(1_000_000);
    expect(snap.cursorAccountUsage?.total_tokens).toBe(900_000);
  });

  it("drops a stale post-Cursor-refresh overview reload when the user changes filters before SQLite finishes", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();

    backend.refreshedCursorEvents = [
      { occurred_at: "2026-08-16T15:00:00.000Z", model: "gpt-5", tokens: 700_000 },
    ];
    const staleBundleGate = backend.gateNext("getOverviewBundle");
    const cursorRefresh = controller.refreshCursorAccount();
    await new Promise((resolve) => setTimeout(resolve, 0));

    // User switches filter to claude-opus-4 while Cursor's overview reload is still pending
    await controller.applyFilter({
      ...controller.getSnapshot().filter,
      models: ["claude-opus-4"],
    });
    expect(controller.getSnapshot().overview?.total_tokens).toBe(1_000_000);

    // Now the older Cursor-triggered unfiltered reload finally resolves — must be dropped!
    staleBundleGate.resolve();
    await cursorRefresh;

    expect(controller.getSnapshot().filter.models).toEqual(["claude-opus-4"]);
    expect(controller.getSnapshot().overview?.total_tokens).toBe(1_000_000);
    expect(controller.getSnapshot().cursorAccountUsage?.total_tokens).toBe(0);
  });

  it("does not clobber a non-Cursor view during background Cursor refresh and shares single-flight pull", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();

    const refreshGate = backend.gateNext("refreshCursorAccountUsage");
    backend.refreshedCursorEvents = [
      { occurred_at: "2026-08-16T15:00:00.000Z", model: "gpt-5", tokens: 800_000 },
    ];
    const first = controller.refreshCursorAccount();
    const second = controller.refreshCursorAccount();
    expect(backend.count("refreshCursorAccountUsage")).toBe(1);

    await controller.navigate("model");
    backend.resetCounts();

    refreshGate.resolve();
    await Promise.all([first, second]);

    // Model view was not needlessly re-fetched or clobbered
    expect(backend.count("getBreakdown")).toBe(0);
    expect(backend.count("getOverviewBundle")).toBe(0);

    // Navigating back to overview re-fetches because cursor revision bumped
    await controller.navigate("overview");
    expect(backend.count("getOverviewBundle")).toBe(1);
    expect(controller.getSnapshot().cursorAccountUsage?.total_tokens).toBe(800_000);
  });
});

describe("UsageViewController — grain switches, concurrency, ingest, and range history", () => {
  it("uses targeted getTrend for grain changes on idle overview and keeps overview warm", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();
    backend.resetCounts();

    await controller.setGrain("month");
    expect(backend.count("getTrend")).toBe(1);
    expect(backend.count("getOverviewBundle")).toBe(0);
    expect(controller.getSnapshot().grain).toBe("month");
    expect(controller.getSnapshot().trend[0]?.bucket).toBe("2026-08");

    // Overview freshness was rebased to grain="month": navigating away and back stays warm
    await controller.navigate("trend");
    await controller.navigate("overview");
    expect(backend.count("getOverviewBundle")).toBe(0);
  });

  it("upgrades grain change to a wide refresh while an overview refresh is in flight", async () => {
    const backend = new StatefulUsageBackendFake();
    const firstQuotaGate = backend.gateNext("getOfficialQuota");
    const controller = makeController(backend, "overview");

    const hanging = controller.refreshActiveView();
    await new Promise((resolve) => setTimeout(resolve, 0));

    // Switch grain while initial wide refresh is still pending
    await controller.setGrain("month");
    expect(backend.count("getOverviewBundle")).toBe(2);
    expect(controller.getSnapshot().trend[0]?.bucket).toBe("2026-08");
    expect(controller.getSnapshot().overview?.total_tokens).toBe(1_500_000);

    firstQuotaGate.resolve();
    await hanging;

    // Now that the wide refresh completed, the next grain switch is narrow getTrend only
    backend.resetCounts();
    await controller.setGrain("hour");
    expect(backend.count("getTrend")).toBe(1);
    expect(backend.count("getOverviewBundle")).toBe(0);
  });

  it("ignores grain changes on grain-insensitive views without cancelling in-flight queries", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "model");
    const breakdownGate = backend.gateNext("getBreakdown");

    const pending = controller.refreshActiveView();
    await controller.setGrain("week");
    expect(backend.count("getTrend")).toBe(0);

    breakdownGate.resolve();
    await pending;
    expect(controller.getSnapshot().models.map((m) => m.name)).toEqual(["claude-opus-4", "gpt-5"]);
  });

  it("runs ingest single-flight, bumps data epoch & sessionsRevision, and updates active view", async () => {
    const backend = new StatefulUsageBackendFake();
    let detailCleared = 0;
    const controller = new UsageViewController({
      backend,
      initialView: "overview",
      initialPreset: "all",
      onClearSessionDetailCache: () => {
        detailCleared += 1;
      },
    });
    await controller.refreshActiveView();
    const sessionsBefore = controller.getSnapshot().sessionsRevision;

    const ingestGate = backend.gateNext("ingest");
    backend.state.records.push({
      occurred_at: "2026-08-17T08:00:00.000Z",
      source: "pi",
      model: "claude-opus-4",
      provider: "anthropic",
      project: "/new",
      session_id: "sess-3",
      tokens: 250_000,
    });
    backend.failMethod.refreshTray = new Error("tray offline");

    const first = controller.runIngest("刷新");
    const second = controller.runIngest("刷新");
    expect(backend.count("ingest")).toBe(1);

    ingestGate.resolve();
    await Promise.all([first, second]);

    expect(detailCleared).toBe(1);
    expect(controller.getSnapshot().sessionsRevision).toBe(sessionsBefore + 1);
    expect(controller.getSnapshot().overview?.total_tokens).toBe(1_750_000);
    expect(controller.getSnapshot().options.sources).toContain("pi");
    expect(controller.getSnapshot().busy).toBe(false);
  });

  it("tracks drill-down range history and pops back cleanly, clearing on applyPreset", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();
    expect(controller.getSnapshot().canGoBack).toBe(false);

    await controller.drillRange("2026-08-15", "2026-08-15");
    expect(controller.getSnapshot().canGoBack).toBe(true);
    expect(controller.getSnapshot().preset).toBe("custom");
    expect(controller.getSnapshot().overview?.total_tokens).toBe(1_000_000);

    await controller.popRange();
    expect(controller.getSnapshot().canGoBack).toBe(false);
    expect(controller.getSnapshot().preset).toBe("all");
    expect(controller.getSnapshot().overview?.total_tokens).toBe(1_500_000);

    await controller.drillRange("2026-08-15", "2026-08-15");
    expect(controller.getSnapshot().canGoBack).toBe(true);
    await controller.applyPreset("all");
    expect(controller.getSnapshot().canGoBack).toBe(false);
  });

  it("defaults every view scope to the last 7 days and isolates conversation source filters", () => {
    const scope = emptyViewScope();
    expect(scope.preset).toBe("7");
    expect(Date.parse(scope.filter.to!) - Date.parse(scope.filter.from!)).toBe(7 * 24 * 3600 * 1000);

    const scopes = initialViewScopes();
    const updated = syncSharedFilters(
      scopes,
      { ...scopes.overview.filter, sources: ["claude"], models: ["opus"], projects: ["/app"] },
      "7",
      "overview",
    );
    expect(updated.trend.filter.sources).toEqual(["claude"]);
    expect(updated.conversations.filter.sources).toEqual([]);
    expect(updated.conversations.filter.models).toEqual(["opus"]);

    const fromConversations = syncSharedFilters(
      updated,
      { ...updated.conversations.filter, sources: ["cursor_agent"], models: ["gpt-5"] },
      "7",
      "conversations",
    );
    expect(fromConversations.conversations.filter.sources).toEqual(["cursor_agent"]);
    expect(fromConversations.overview.filter.sources).toEqual(["claude"]);
    expect(fromConversations.overview.filter.models).toEqual(["gpt-5"]);
    expect(
      filtersEqual(
        { ...scope.filter, projects: ["/a", "/b"] },
        { ...scope.filter, projects: ["/b", "/a"] },
      ),
    ).toBe(true);
  });

  it("clears Cursor cache in-place when active on overview and cancels in-flight pre-clear queries", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "overview");
    await controller.refreshActiveView();

    const preClearGate = backend.gateNext("getOverviewBundle");
    const staleRefresh = controller.refreshActiveView();
    await new Promise((resolve) => setTimeout(resolve, 0));

    await controller.clearCursorAccountUsage();
    expect(controller.getSnapshot().cursorAccountUsage?.event_count).toBe(0);
    expect(controller.getSnapshot().projects.some((p) => p.name === "Cursor（账号用量）")).toBe(
      false,
    );

    // Pre-clear overview refresh resolves late — must NOT restore stale Cursor rows
    preClearGate.resolve();
    await staleRefresh;
    expect(controller.getSnapshot().cursorAccountUsage?.event_count).toBe(0);
  });

  it("runs rebuild and purgeArchived single-flight and clears loading on view refresh errors", async () => {
    const backend = new StatefulUsageBackendFake();
    const controller = makeController(backend, "trend");
    await controller.refreshActiveView();
    const revBefore = controller.getSnapshot().sessionsRevision;

    await controller.runRebuild("claude");
    expect(backend.count("rebuildCache")).toBe(1);
    expect(controller.getSnapshot().rebuilding).toBeNull();
    expect(controller.getSnapshot().sessionsRevision).toBe(revBefore + 1);

    await controller.runPurgeArchived(null);
    expect(backend.count("purgeArchivedRecords")).toBe(1);
    expect(controller.getSnapshot().purging).toBeNull();
    expect(controller.getSnapshot().sessionsRevision).toBe(revBefore + 2);

    backend.failMethod.getTrend = new Error("sqlite busy");
    await expect(controller.refreshActiveView()).rejects.toThrow("sqlite busy");
    expect(controller.getSnapshot().loading).toBe(false);
  });
});
