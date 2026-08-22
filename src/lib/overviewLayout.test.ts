import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  applyDetectedQuotaSources,
  applyFavoriteQuotaSources,
  collectPresentSources,
  defaultOverviewLayout,
  filterOfficialQuotaRows,
  filterQuotaItems,
  isModuleVisible,
  isOfficialProviderVisible,
  isQuotaSourceVisible,
  officialQuotaProviderLabel,
  OFFICIAL_QUOTA_PROVIDER_IDS,
  OVERVIEW_LAYOUT_STORAGE_KEY,
  OVERVIEW_MODULE_IDS,
  parseOverviewLayout,
  QUOTA_SOURCE_IDS,
  quotaSourceChipIds,
  readOverviewLayout,
  setAllModulesVisible,
  setAllOfficialProvidersVisible,
  setAllQuotaSourcesVisible,
  setModuleVisible,
  setOfficialProviderVisible,
  setQuotaSourceVisible,
  summarizeOverviewLayout,
  visibleModuleCount,
  visibleOfficialProviderCount,
  visibleOfficialQuotaRows,
  visibleQuotaSourceCount,
  writeOverviewLayout,
} from "./overviewLayout";

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

describe("parseOverviewLayout", () => {
  it("returns all-visible defaults for empty input", () => {
    const layout = parseOverviewLayout(null);
    expect(layout).toEqual(defaultOverviewLayout());
    expect(OVERVIEW_MODULE_IDS.every((id) => layout.modules[id])).toBe(true);
    expect(QUOTA_SOURCE_IDS.every((id) => layout.quotaSources[id])).toBe(true);
    expect(OFFICIAL_QUOTA_PROVIDER_IDS.every((id) => layout.officialProviders[id])).toBe(true);
    expect(visibleOfficialProviderCount(layout)).toBe(OFFICIAL_QUOTA_PROVIDER_IDS.length);
    expect(OFFICIAL_QUOTA_PROVIDER_IDS).toEqual([
      "claude",
      "codex",
      "cursor",
      "grok",
      "droid",
      "antigravity",
      "opencode",
      "copilot",
      "devin",
    ]);
  });

  it("merges partial stored config and keeps unknown sources", () => {
    const layout = parseOverviewLayout(
      JSON.stringify({
        modules: { heatmap: false, kpi: true },
        quotaSources: { codex: true, claude: false, custom_src: false },
      }),
    );
    expect(layout.modules.heatmap).toBe(false);
    expect(layout.modules.official).toBe(true);
    expect(layout.modules.cursorAccount).toBe(true);
    expect(layout.modules.billing).toBe(true);
    expect(layout.quotaSources.codex).toBe(true);
    expect(layout.quotaSources.claude).toBe(false);
    expect(layout.quotaSources.cursor_agent).toBe(true);
    expect(layout.quotaSources.custom_src).toBe(false);
    expect(layout.officialProviders.claude).toBe(true);
    expect(layout.officialProviders.codex).toBe(true);
    expect(layout.officialProviders.droid).toBe(true);
    expect(layout.officialProviders.antigravity).toBe(true);
    expect(layout.officialProviders.opencode).toBe(true);
    expect(layout.officialProviders.copilot).toBe(true);
    expect(layout.officialProviders.devin).toBe(true);
  });

  it("reads official provider flags independently of billing sources", () => {
    const layout = parseOverviewLayout(
      JSON.stringify({
        officialProviders: { claude: false, cursor: true },
        quotaSources: { claude: true },
      }),
    );
    expect(layout.officialProviders.claude).toBe(false);
    expect(layout.officialProviders.codex).toBe(true);
    expect(layout.officialProviders.cursor).toBe(true);
    expect(layout.officialProviders.grok).toBe(true);
    expect(layout.officialProviders.droid).toBe(true);
    expect(layout.quotaSources.claude).toBe(true);
  });

  it("keeps extra official account flags so newly added providers can be hidden", () => {
    const layout = parseOverviewLayout(
      JSON.stringify({
        officialProviders: { droid: false, antigravity: false, custom_acct: false },
      }),
    );
    expect(layout.officialProviders.droid).toBe(false);
    expect(layout.officialProviders.antigravity).toBe(false);
    expect(layout.officialProviders.custom_acct).toBe(false);
    expect(layout.officialProviders.claude).toBe(true);
  });

  it("falls back for invalid JSON or non-object payloads", () => {
    expect(parseOverviewLayout("{")).toEqual(defaultOverviewLayout());
    expect(parseOverviewLayout("[]")).toEqual(defaultOverviewLayout());
    expect(parseOverviewLayout('"nope"')).toEqual(defaultOverviewLayout());
  });
});

describe("visibility helpers", () => {
  it("treats missing flags as visible", () => {
    const layout = defaultOverviewLayout();
    layout.quotaSources = { codex: false };
    expect(isModuleVisible(layout, "weekly")).toBe(true);
    expect(isQuotaSourceVisible(layout, "codex")).toBe(false);
    expect(isQuotaSourceVisible(layout, "cursor_agent")).toBe(true);
    expect(isQuotaSourceVisible(layout, "unknown")).toBe(true);
  });

  it("filters quota rows by configured sources", () => {
    const layout = setQuotaSourceVisible(defaultOverviewLayout(), "claude", false);
    const rows = filterQuotaItems(
      [
        { source: "codex", total: 1 },
        { source: "claude", total: 2 },
        { source: "cursor_agent", total: 3 },
      ],
      layout,
    );
    expect(rows.map((row) => row.source)).toEqual(["codex", "cursor_agent"]);
  });

  it("filters official quota rows by selected accounts", () => {
    const layout = setOfficialProviderVisible(
      setOfficialProviderVisible(defaultOverviewLayout(), "claude", false),
      "droid",
      false,
    );
    const rows = filterOfficialQuotaRows(
      [
        { provider: "codex" },
        { provider: "claude" },
        { provider: "cursor" },
        { provider: "droid" },
        { provider: "antigravity" },
      ],
      layout,
    );
    expect(rows.map((row) => row.provider)).toEqual(["codex", "cursor", "antigravity"]);
    expect(isOfficialProviderVisible(layout, "claude")).toBe(false);
    expect(isOfficialProviderVisible(layout, "droid")).toBe(false);
    expect(isOfficialProviderVisible(layout, "unknown")).toBe(true);
    expect(officialQuotaProviderLabel("claude")).toBe("Claude Code");
    expect(officialQuotaProviderLabel("devin")).toBe("Devin");
    expect(officialQuotaProviderLabel("custom_acct")).toBe("custom_acct");
  });

  it("filters tray quota rows by hidden_providers without touching the rest", () => {
    const rows = [{ provider: "claude" }, { provider: "cursor" }, { provider: "grok" }];
    expect(visibleOfficialQuotaRows(rows, undefined)).toEqual(rows);
    expect(visibleOfficialQuotaRows(rows, [])).toEqual(rows);
    expect(visibleOfficialQuotaRows(rows, ["claude", "grok"]).map((row) => row.provider)).toEqual([
      "cursor",
    ]);
  });

  it("toggles modules and sources without mutating the original", () => {
    const original = defaultOverviewLayout();
    const hiddenHeatmap = setModuleVisible(original, "heatmap", false);
    const hiddenCursorAccount = setModuleVisible(original, "cursorAccount", false);
    const hiddenCodex = setQuotaSourceVisible(original, "codex", false);
    expect(original.modules.heatmap).toBe(true);
    expect(original.modules.cursorAccount).toBe(true);
    expect(original.quotaSources.codex).toBe(true);
    expect(hiddenHeatmap.modules.heatmap).toBe(false);
    expect(hiddenCursorAccount.modules.cursorAccount).toBe(false);
    expect(hiddenCodex.quotaSources.codex).toBe(false);
  });

  it("supports show/hide all and counts visible items", () => {
    const hidden = setAllModulesVisible(defaultOverviewLayout(), false);
    const shown = setAllQuotaSourcesVisible(setAllQuotaSourcesVisible(hidden, false), true);
    expect(visibleModuleCount(hidden)).toBe(0);
    expect(visibleQuotaSourceCount(shown)).toBe(QUOTA_SOURCE_IDS.length);
    expect(shown.modules.kpi).toBe(false);
    const onlyCodex = setAllOfficialProvidersVisible(
      setOfficialProviderVisible(defaultOverviewLayout(), "claude", false),
      false,
    );
    expect(visibleOfficialProviderCount(setOfficialProviderVisible(onlyCodex, "codex", true))).toBe(
      1,
    );
  });

  it("applies favorite and detected source sets", () => {
    const favorites = applyFavoriteQuotaSources(defaultOverviewLayout());
    expect(favorites.quotaSources.codex).toBe(true);
    expect(favorites.quotaSources.cursor).toBe(true);
    expect(favorites.quotaSources.cursor_agent).toBe(true);
    expect(favorites.quotaSources.grok).toBe(false);
    const detected = applyDetectedQuotaSources(favorites, ["codex", "kimi"]);
    expect(detected.quotaSources.codex).toBe(true);
    expect(detected.quotaSources.cursor_agent).toBe(false);
    expect(detected.quotaSources.kimi).toBe(true);
  });

  it("lists present sources and collapses chips until show-all", () => {
    const present = collectPresentSources(["kimi"], [{ source: "codex" }, { source: "custom" }]);
    expect(present).toEqual(["codex", "kimi", "custom"]);
    expect(quotaSourceChipIds(present, false)).toEqual(["codex", "kimi", "custom"]);
    expect(quotaSourceChipIds(present, true)[0]).toBe("codex");
    expect(quotaSourceChipIds(present, true)).toContain("cursor_agent");
    expect(quotaSourceChipIds([], false)).toEqual([...QUOTA_SOURCE_IDS]);
  });

  it("summarizes hidden modules and present sources", () => {
    const layout = setModuleVisible(
      setQuotaSourceVisible(defaultOverviewLayout(), "claude", false),
      "heatmap",
      false,
    );
    const summary = summarizeOverviewLayout(layout, ["codex", "claude", "cursor_agent"]);
    expect(summary.hiddenModules).toEqual(["heatmap"]);
    expect(summary.hiddenPresentSources).toEqual(["claude"]);
    expect(summary.hiddenOfficialProviders).toEqual([]);
    const official = setOfficialProviderVisible(
      setOfficialProviderVisible(layout, "cursor", false),
      "devin",
      false,
    );
    expect(summarizeOverviewLayout(official).hiddenOfficialProviders).toEqual(["cursor", "devin"]);
  });
});

describe("readOverviewLayout / writeOverviewLayout", () => {
  beforeEach(() => {
    installMemoryStorage();
  });

  afterEach(() => {
    localStorage.removeItem(OVERVIEW_LAYOUT_STORAGE_KEY);
  });

  it("persists and reloads a customized layout", () => {
    const next = setModuleVisible(
      setQuotaSourceVisible(defaultOverviewLayout(), "cursor_agent", false),
      "billing",
      false,
    );
    writeOverviewLayout(next);
    const loaded = readOverviewLayout();
    expect(loaded.modules.billing).toBe(false);
    expect(loaded.quotaSources.cursor_agent).toBe(false);
    expect(loaded.modules.weekly).toBe(true);
  });
});
