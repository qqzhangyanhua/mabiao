import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AUTO_REFRESH_STORAGE_KEY,
  CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES,
  CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY,
} from "./usage/constants";
import { StatefulUsageBackendFake } from "./usage/usageBackendFake";
import { useUsageData } from "./useUsageData";

async function flushAsync() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("useUsageData hook seam", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.history.replaceState(null, "", "#overview");
  });

  afterEach(() => {
    vi.useRealTimers();
    window.localStorage.clear();
    window.history.replaceState(null, "", "#overview");
  });

  it("pings, loads overview bundle, runs startup ingest, and never pulls Cursor account on startup", async () => {
    const backend = new StatefulUsageBackendFake();
    const { result } = renderHook(() => useUsageData({ backend, initialPreset: "all" }));

    await flushAsync();

    expect(result.current.connected).toBe(true);
    expect(backend.count("ping")).toBe(1);
    expect(backend.count("ingest")).toBe(1);
    expect(backend.count("getOverviewBundle")).toBeGreaterThanOrEqual(1);
    expect(backend.count("refreshCursorAccountUsage")).toBe(0);
    expect(result.current.overview?.total_tokens).toBe(1_500_000);
    expect(result.current.overview?.cost).toBeCloseTo(2.5, 6);
  });

  it("preserves ADR-0006 independent 10-minute localStorage Cursor auto-refresh timer", async () => {
    vi.useFakeTimers();
    const backend = new StatefulUsageBackendFake();
    const { result } = renderHook(() => useUsageData({ backend, initialPreset: "all" }));
    await flushAsync();

    expect(result.current.cursorAccountAutoRefresh).toBe(false);
    expect(window.localStorage.getItem(CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY)).toBe("off");
    expect(window.localStorage.getItem(AUTO_REFRESH_STORAGE_KEY)).toBe("off");

    backend.resetCounts();
    backend.refreshedCursorEvents = [
      { occurred_at: "2026-08-16T15:00:00.000Z", model: "gpt-5", tokens: 600_000 },
    ];

    // Turning on Cursor auto-refresh triggers an immediate pull (ADR-0006)
    act(() => {
      result.current.setCursorAccountAutoRefresh(true);
    });
    await flushAsync();

    expect(window.localStorage.getItem(CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY)).toBe("on");
    expect(backend.count("refreshCursorAccountUsage")).toBe(1);
    expect(backend.count("ingest")).toBe(0);
    expect(result.current.cursorAccountUsage?.total_tokens).toBe(600_000);

    // Advance 10 minutes: triggers periodic Cursor pull without running local ingest
    await act(async () => {
      await vi.advanceTimersByTimeAsync(CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES * 60_000);
    });
    expect(backend.count("refreshCursorAccountUsage")).toBe(2);
    expect(backend.count("ingest")).toBe(0);

    // Turning off stops further timer ticks
    act(() => {
      result.current.setCursorAccountAutoRefresh(false);
    });
    expect(window.localStorage.getItem(CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY)).toBe("off");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES * 60_000);
    });
    expect(backend.count("refreshCursorAccountUsage")).toBe(2);
  });

  it("propagates savePrices, LiteLLM snapshot invalidation, Cursor clear, and backup restore through React state without disk ingest", async () => {
    const backend = new StatefulUsageBackendFake();
    const { result } = renderHook(() => useUsageData({ backend, initialPreset: "all" }));
    await flushAsync();
    backend.resetCounts();

    // 1. Save user price from Settings -> Overview immediately reflects new cost
    act(() => {
      result.current.navigate("settings");
    });
    await flushAsync();

    act(() => {
      result.current.setPrices({
        prices: [
          {
            model: "claude-opus-4",
            provider: null,
            input: 8 / 1_000_000,
            output: 0,
            cache_read: 0,
            cache_creation: 0,
            origin: "user",
          },
        ],
      });
    });
    await act(async () => {
      await result.current.savePrices();
    });
    expect(backend.count("ingest")).toBe(0);
    expect(result.current.budgetStatus?.month_to_date_cost).toBeCloseTo(8.5, 6);

    act(() => {
      result.current.navigate("overview");
    });
    await flushAsync();
    expect(result.current.overview?.cost).toBeCloseTo(8.5, 6);

    // 2. LiteLLM snapshot refresh invalidates pricing without ingest
    backend.applySnapshotPrices([
      {
        model: "gpt-5",
        provider: null,
        input: 4 / 1_000_000,
        output: 0,
        cache_read: 0,
        cache_creation: 0,
        origin: "snapshot",
      },
    ]);
    await act(async () => {
      await result.current.invalidatePricing();
    });
    expect(backend.count("ingest")).toBe(0);
    expect(result.current.overview?.cost).toBeCloseTo(10.0, 6);

    // 3. Clearing Cursor account cache immediately removes ghosts
    const cursorRevBefore = result.current.cursorAccountRevision;
    await act(async () => {
      await result.current.clearCursorAccountUsage();
    });
    expect(result.current.cursorAccountRevision).toBe(cursorRevBefore + 1);
    expect(result.current.cursorAccountUsage?.event_count).toBe(0);
    expect(result.current.projects.some((p) => p.name === "Cursor（账号用量）")).toBe(false);

    // 4. Backup restore reloads all restored state without ingest
    const restoreRevBefore = result.current.restoreRevision;
    backend.applyBackupSnapshot({
      ...backend.state,
      records: [
        {
          occurred_at: "2026-07-10T10:00:00.000Z",
          source: "codex",
          model: "gpt-5",
          provider: "openai",
          project: "/restored",
          session_id: "s-restored",
          tokens: 1_000_000,
        },
      ],
      budget: { monthly_usd: 77 },
    });
    await act(async () => {
      await result.current.reloadAfterRestore();
    });
    expect(backend.count("ingest")).toBe(0);
    expect(result.current.restoreRevision).toBe(restoreRevBefore + 1);
    expect(result.current.budgetStatus?.monthly_budget).toBe(77);
    expect(result.current.overview?.total_tokens).toBe(1_000_000);
  });
});
