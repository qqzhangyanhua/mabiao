import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { OFFICIAL_QUOTA_PROVIDER_SLUGS, SOURCE_SLUGS } from "../generated/displayIdentity";
import {
  acctProviderSlug,
  applyDetectedQuotaSources,
  applyFavoriteQuotaSources,
  collectPresentSources,
  defaultOverviewLayout,
  extractLegacyHiddenOfficialProviders,
  filterQuotaItems,
  isModuleVisible,
  isOfficialProviderVisible,
  isQuotaSourceVisible,
  OFFICIAL_QUOTA_PROVIDER_IDS,
  officialQuotaAttributionId,
  officialQuotaProviderLabel,
  OVERVIEW_LAYOUT_STORAGE_KEY,
  OVERVIEW_MODULE_IDS,
  parseOverviewLayout,
  QUOTA_SOURCE_IDS,
  quotaSourceChipIds,
  readOverviewLayout,
  sanitizeHiddenOfficialProviders,
  setAllModulesVisible,
  setAllOfficialProvidersVisible,
  setAllQuotaSourcesVisible,
  setModuleVisible,
  setOfficialProviderVisible,
  setQuotaSourceVisible,
  stripLegacyOfficialProvidersFromStorage,
  summarizeOverviewLayout,
  visibleModuleCount,
  visibleOfficialProviderCount,
  visibleOfficialQuotaRows,
  visibleQuotaSourceCount,
  visibleUndetectedOfficialProviders,
  writeOverviewLayout,
} from "./overviewLayout";

function installMemoryStorage(): Storage {
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
  return memory;
}

describe("parseOverviewLayout", () => {
  it("returns all-visible defaults for empty input without officialProviders", () => {
    const layout = parseOverviewLayout(null);
    expect(layout).toEqual(defaultOverviewLayout());
    expect("officialProviders" in layout).toBe(false);
    expect(OVERVIEW_MODULE_IDS.every((id) => layout.modules[id])).toBe(true);
    expect(QUOTA_SOURCE_IDS.every((id) => layout.quotaSources[id])).toBe(true);
    expect(visibleOfficialProviderCount([])).toBe(OFFICIAL_QUOTA_PROVIDER_IDS.length);
    expect(OFFICIAL_QUOTA_PROVIDER_IDS).toBe(OFFICIAL_QUOTA_PROVIDER_SLUGS);
  });

  it("merges partial stored config, keeps unknown sources, and ignores legacy officialProviders", () => {
    const layout = parseOverviewLayout(
      JSON.stringify({
        modules: { heatmap: false, kpi: true },
        quotaSources: { codex: true, claude: false, custom_src: false },
        officialProviders: { claude: false, droid: false },
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
    expect("officialProviders" in layout).toBe(false);
  });

  it("falls back for invalid JSON or non-object payloads", () => {
    expect(parseOverviewLayout("{")).toEqual(defaultOverviewLayout());
    expect(parseOverviewLayout("[]")).toEqual(defaultOverviewLayout());
    expect(parseOverviewLayout('"nope"')).toEqual(defaultOverviewLayout());
  });
});

describe("legacy officialProviders migration helpers", () => {
  beforeEach(() => {
    installMemoryStorage();
  });

  afterEach(() => {
    localStorage.removeItem(OVERVIEW_LAYOUT_STORAGE_KEY);
  });

  it("extracts hidden built-in providers and excludes custom:* from legacy storage", () => {
    expect(extractLegacyHiddenOfficialProviders(null)).toEqual({
      hasLegacyField: false,
      hiddenProviders: [],
    });
    expect(
      extractLegacyHiddenOfficialProviders(
        JSON.stringify({ modules: { heatmap: false }, quotaSources: { claude: false } }),
      ),
    ).toEqual({
      hasLegacyField: false,
      hiddenProviders: [],
    });
    expect(
      extractLegacyHiddenOfficialProviders(
        JSON.stringify({
          modules: { heatmap: false },
          officialProviders: {
            claude: false,
            cursor: true,
            droid: false,
            "custom:a3f9c1": false,
          },
        }),
      ),
    ).toEqual({
      hasLegacyField: true,
      hiddenProviders: ["claude", "droid"],
    });
  });

  it("strips officialProviders from storage while preserving modules and quotaSources", () => {
    localStorage.setItem(
      OVERVIEW_LAYOUT_STORAGE_KEY,
      JSON.stringify({
        modules: { heatmap: false },
        quotaSources: { claude: false },
        officialProviders: { claude: false },
      }),
    );

    expect(stripLegacyOfficialProvidersFromStorage()).toBe(true);
    const stored = JSON.parse(
      localStorage.getItem(OVERVIEW_LAYOUT_STORAGE_KEY) ?? "{}",
    ) as Record<string, unknown>;
    expect(stored).toEqual({
      modules: { heatmap: false },
      quotaSources: { claude: false },
    });
    expect(stripLegacyOfficialProvidersFromStorage()).toBe(false);
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

  it("filters official quota rows and undetected providers by persisted hidden_providers", () => {
    const hidden = setOfficialProviderVisible(
      setOfficialProviderVisible([], "claude", false),
      "droid",
      false,
    );
    const rows = visibleOfficialQuotaRows(
      [
        { provider: "codex" },
        { provider: "claude" },
        { provider: "cursor" },
        { provider: "droid" },
        { provider: "antigravity" },
      ],
      hidden,
    );
    expect(rows.map((row) => row.provider)).toEqual(["codex", "cursor", "antigravity"]);
    expect(
      visibleUndetectedOfficialProviders(["claude", "grok", "droid", "devin"], hidden),
    ).toEqual(["grok", "devin"]);
    expect(isOfficialProviderVisible(hidden, "claude")).toBe(false);
    expect(isOfficialProviderVisible(hidden, "droid")).toBe(false);
    expect(isOfficialProviderVisible(hidden, "unknown")).toBe(true);
  });

  it("labels built-in official quota providers with display identity names", () => {
    expect(officialQuotaProviderLabel("grok")).toBe("Grok CLI");
    expect(officialQuotaProviderLabel("copilot")).toBe("GitHub Copilot CLI");
    expect(officialQuotaProviderLabel("claude")).toBe("Claude Code");
    expect(officialQuotaProviderLabel("devin")).toBe("Devin");
    expect(officialQuotaProviderLabel("custom_acct")).toBe("custom_acct");
  });

  it("filters quota rows by hidden_providers without touching the rest", () => {
    const rows = [{ provider: "claude" }, { provider: "cursor" }, { provider: "grok" }];
    expect(visibleOfficialQuotaRows(rows, undefined)).toEqual(rows);
    expect(visibleOfficialQuotaRows(rows, [])).toEqual(rows);
    expect(visibleOfficialQuotaRows(rows, ["claude", "grok"]).map((row) => row.provider)).toEqual([
      "cursor",
    ]);
  });

  it("keeps custom providers visible even when every built-in account or custom:* appears in hidden_providers", () => {
    expect(OFFICIAL_QUOTA_PROVIDER_IDS.some((id) => id.startsWith("custom:"))).toBe(false);
    const rows = [
      { provider: "claude" },
      { provider: "cursor" },
      { provider: "custom:a3f9c1" },
    ];
    const noisyHidden = ["claude", "custom:a3f9c1", "claude", "  "];
    expect(sanitizeHiddenOfficialProviders(noisyHidden)).toEqual(["claude"]);
    expect(isOfficialProviderVisible(noisyHidden, "custom:a3f9c1")).toBe(true);
    expect(visibleOfficialQuotaRows(rows, noisyHidden).map((row) => row.provider)).toEqual([
      "cursor",
      "custom:a3f9c1",
    ]);
    expect(
      summarizeOverviewLayout(defaultOverviewLayout(), [], noisyHidden).hiddenOfficialProviders,
    ).toEqual(["claude"]);
    const hiddenAll = setAllOfficialProvidersVisible(false);
    expect(isOfficialProviderVisible(hiddenAll, "custom:a3f9c1")).toBe(true);
    expect(visibleOfficialQuotaRows(rows, hiddenAll).map((row) => row.provider)).toEqual([
      "custom:a3f9c1",
    ]);
    expect(
      summarizeOverviewLayout(defaultOverviewLayout(), [], hiddenAll).hiddenOfficialProviders,
    ).not.toContain("custom:a3f9c1");
  });

  it("hides quota-account rows with the built-in provider and keeps custom rows", () => {
    const hiddenCodex = setOfficialProviderVisible([], "codex", false);
    const rows = [
      { provider: "codex" },
      { provider: "acct:codex:ab12" },
      { provider: "acct:codex:cd34", base_provider: "codex" },
      { provider: "acct:codex:ee56", base_provider: "claude" },
      { provider: "claude" },
      { provider: "custom:a3f9c1", base_provider: "codex" },
    ];
    expect(visibleOfficialQuotaRows(rows, hiddenCodex).map((row) => row.provider)).toEqual([
      "acct:codex:ee56",
      "claude",
      "custom:a3f9c1",
    ]);
    expect(isOfficialProviderVisible(hiddenCodex, "acct:codex:ab12")).toBe(false);
    expect(acctProviderSlug("acct:codex:ab12")).toBe("codex");
    expect(acctProviderSlug("custom:a3f9c1")).toBeNull();
    expect(
      officialQuotaAttributionId({ provider: "custom:a3f9c1", base_provider: "codex" }),
    ).toBe("custom:a3f9c1");
    expect(
      officialQuotaAttributionId({ provider: "acct:codex:ee56", base_provider: " claude " }),
    ).toBe("claude");

    const hiddenAll = setAllOfficialProvidersVisible(false);
    expect(visibleOfficialQuotaRows(rows, hiddenAll).map((row) => row.provider)).toEqual([
      "custom:a3f9c1",
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
    const allHidden = setAllOfficialProvidersVisible(false);
    expect(visibleOfficialProviderCount(setOfficialProviderVisible(allHidden, "codex", true))).toBe(
      1,
    );
    expect(setAllOfficialProvidersVisible(true)).toEqual([]);
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

  it("summarizes hidden modules, present sources, and official providers", () => {
    const layout = setModuleVisible(
      setQuotaSourceVisible(defaultOverviewLayout(), "claude", false),
      "heatmap",
      false,
    );
    const summary = summarizeOverviewLayout(layout, ["codex", "claude", "cursor_agent"], []);
    expect(summary.hiddenModules).toEqual(["heatmap"]);
    expect(summary.hiddenPresentSources).toEqual(["claude"]);
    expect(summary.hiddenOfficialProviders).toEqual([]);
    expect(
      summarizeOverviewLayout(layout, [], ["devin", "cursor"]).hiddenOfficialProviders,
    ).toEqual(["cursor", "devin"]);
  });
});

describe("readOverviewLayout / writeOverviewLayout", () => {
  beforeEach(() => {
    installMemoryStorage();
  });

  afterEach(() => {
    localStorage.removeItem(OVERVIEW_LAYOUT_STORAGE_KEY);
  });

  it("persists and reloads a customized layout without writing officialProviders", () => {
    const next = setModuleVisible(
      setQuotaSourceVisible(defaultOverviewLayout(), "cursor_agent", false),
      "billing",
      false,
    );
    writeOverviewLayout(next);
    const raw = JSON.parse(
      localStorage.getItem(OVERVIEW_LAYOUT_STORAGE_KEY) ?? "{}",
    ) as Record<string, unknown>;
    expect("officialProviders" in raw).toBe(false);
    const loaded = readOverviewLayout();
    expect(loaded.modules.billing).toBe(false);
    expect(loaded.quotaSources.cursor_agent).toBe(false);
    expect(loaded.modules.weekly).toBe(true);
  });
});

describe("QUOTA_SOURCE_IDS", () => {
  it("equals every usage source plus cursor as a set", () => {
    expect([...QUOTA_SOURCE_IDS].sort()).toEqual([...SOURCE_SLUGS, "cursor"].sort());
  });
});
