import { beforeEach, describe, expect, it, vi } from "vitest";
import type { View } from "../../types";
import { emptyFilter } from "./constants";
import type { UsageViewPatch } from "./useUsageViewState";
import type { ViewRefreshContext } from "./viewRefresh";

const invokeMock = vi.fn<(...args: unknown[]) => Promise<unknown>>();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const { runViewRefresh } = await import("./viewRefresh");

const emptyOptions = { sources: [], models: [], projects: [], providers: [] };

function commandNames(): unknown[] {
  return invokeMock.mock.calls.map((call) => call[0]);
}

function makeContext(overrides: Partial<ViewRefreshContext> = {}): ViewRefreshContext {
  return {
    view: "overview",
    grain: "day",
    hydratedViews: new Set<View>(),
    requestGenerationRef: { current: 0 },
    dataEpochRef: { current: 0 },
    loadedStampsRef: { current: {} },
    optionsEpochRef: { current: -1 },
    markHydrated: vi.fn(),
    apply: vi.fn<(patch: UsageViewPatch) => void>(),
    nextFilter: emptyFilter,
    nextPreset: "7",
    ...overrides,
  };
}

describe("runViewRefresh", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation((command) => {
      if (command === "get_filter_options") {
        return Promise.resolve(emptyOptions);
      }
      return Promise.resolve({});
    });
  });

  it("sends no commands besides filter options for the conversations view", async () => {
    await runViewRefresh(makeContext({ view: "conversations" }));
    expect(commandNames()).toEqual(["get_filter_options"]);
  });
});
