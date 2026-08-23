import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  clampTrayQuotaWindowHeight,
  defaultTrayQuotaLayout,
  quotaProviderFromPoint,
  ensureTrayQuotaOrder,
  isTrayQuotaCollapsed,
  loadTrayQuotaLayout,
  moveTrayQuotaProvider,
  parseTrayQuotaLayout,
  persistTrayQuotaMove,
  saveTrayQuotaLayout,
  sortQuotaRows,
  toggleTrayQuotaCollapsed,
  TRAY_QUOTA_LAYOUT_KEY,
  trayQuotaRowSummary,
} from "./trayQuotaLayout";

function installMemoryStorage() {
  const store = new Map<string, string>();
  const memory: Storage = {
    get length() {
      return store.size;
    },
    clear() {
      store.clear();
    },
    getItem(key) {
      return store.get(key) ?? null;
    },
    key(index) {
      return [...store.keys()][index] ?? null;
    },
    removeItem(key) {
      store.delete(key);
    },
    setItem(key, value) {
      store.set(key, value);
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: memory,
  });
}

describe("parseTrayQuotaLayout", () => {
  it("returns empty defaults for missing or invalid payloads", () => {
    expect(parseTrayQuotaLayout(null)).toEqual(defaultTrayQuotaLayout());
    expect(parseTrayQuotaLayout("{")).toEqual(defaultTrayQuotaLayout());
    expect(parseTrayQuotaLayout("[]")).toEqual(defaultTrayQuotaLayout());
  });

  it("keeps unique string ids only", () => {
    expect(
      parseTrayQuotaLayout(
        JSON.stringify({ order: ["cursor", "cursor", "", 1], collapsed: ["grok", "grok"] }),
      ),
    ).toEqual({ order: ["cursor"], collapsed: ["grok"] });
  });
});

describe("order helpers", () => {
  it("appends unseen providers and drops missing ones", () => {
    expect(ensureTrayQuotaOrder(["cursor", "gone", "claude"], ["claude", "grok", "cursor"])).toEqual([
      "cursor",
      "claude",
      "grok",
    ]);
  });

  it("sorts rows by saved order then appearance", () => {
    const rows = [{ provider: "grok" }, { provider: "cursor" }, { provider: "claude" }];
    expect(sortQuotaRows(rows, ["cursor", "claude"]).map((row) => row.provider)).toEqual([
      "cursor",
      "claude",
      "grok",
    ]);
  });

  it("moves a provider in front of the drop target", () => {
    expect(moveTrayQuotaProvider(["a", "b", "c"], "a", "c")).toEqual(["b", "c", "a"]);
    expect(moveTrayQuotaProvider(["a", "b", "c"], "c", "a")).toEqual(["c", "a", "b"]);
    expect(moveTrayQuotaProvider(["a", "b", "c"], "a", "a")).toEqual(["a", "b", "c"]);
  });

  it("keeps hidden providers after a visible reorder", () => {
    expect(persistTrayQuotaMove(["claude", "cursor", "grok"], ["cursor", "grok"], "grok", "cursor")).toEqual(
      ["grok", "cursor", "claude"],
    );
  });
});

describe("collapse helpers", () => {
  it("toggles a provider in the collapsed list", () => {
    expect(toggleTrayQuotaCollapsed([], "cursor")).toEqual(["cursor"]);
    expect(toggleTrayQuotaCollapsed(["cursor"], "cursor")).toEqual([]);
    expect(isTrayQuotaCollapsed(["cursor"], "cursor")).toBe(true);
    expect(isTrayQuotaCollapsed(["cursor"], "grok")).toBe(false);
  });
});

describe("trayQuotaRowSummary", () => {
  it("uses the tightest window percent when collapsed", () => {
    expect(
      trayQuotaRowSummary({
        windows: [
          { kind: "a", label: "总量", used_percent: 19, resets_at: null },
          { kind: "b", label: "Auto", used_percent: 2, resets_at: null },
        ],
        error: null,
      }),
    ).toBe("19% · 2 窗");
  });

  it("falls back to a short error when there are no windows", () => {
    expect(trayQuotaRowSummary({ windows: [], error: "Copilot 登录已失效，请在编辑器里重新登录" })).toBe(
      "Copilot 登录已失效，请在编…",
    );
  });
});

describe("quotaProviderFromPoint", () => {
  it("returns null when the environment has no hit target", () => {
    expect(quotaProviderFromPoint(0, 0)).toBeNull();
  });
});

describe("clampTrayQuotaWindowHeight", () => {
  it("keeps the popup between the tray min and max", () => {
    expect(clampTrayQuotaWindowHeight(80)).toBe(120);
    expect(clampTrayQuotaWindowHeight(900)).toBe(640);
    expect(clampTrayQuotaWindowHeight(240.4)).toBe(240);
  });
});

describe("loadTrayQuotaLayout / saveTrayQuotaLayout", () => {
  beforeEach(() => {
    installMemoryStorage();
  });

  afterEach(() => {
    localStorage.removeItem(TRAY_QUOTA_LAYOUT_KEY);
  });

  it("round-trips the layout", () => {
    const layout = { order: ["cursor", "grok"], collapsed: ["grok"] };
    saveTrayQuotaLayout(layout);
    expect(loadTrayQuotaLayout()).toEqual(layout);
  });
});
