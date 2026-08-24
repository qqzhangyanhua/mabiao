export const OVERVIEW_LAYOUT_STORAGE_KEY = "mabiao:overview-layout";

export const OVERVIEW_MODULE_IDS = [
  "kpi",
  "official",
  "cursorAccount",
  "billing",
  "weekly",
  "trend",
  "heatmap",
  "detail",
  "status",
] as const;

export type OverviewModuleId = (typeof OVERVIEW_MODULE_IDS)[number];

export const OVERVIEW_MODULE_LABELS: Record<OverviewModuleId, string> = {
  kpi: "指标卡片",
  official: "官方额度",
  cursorAccount: "Cursor 账号用量",
  billing: "5 小时计费窗",
  weekly: "滚动用量",
  trend: "趋势与模型",
  heatmap: "活跃热力图",
  detail: "明细",
  status: "底部状态",
};

/** 额度模块（计费窗 / 滚动用量）可单独开关的来源，常用项靠前。 */
export const QUOTA_SOURCE_IDS = [
  "codex",
  "claude",
  "cursor",
  "cursor_agent",
  "copilot",
  "factory",
  "pi",
  "opencode",
  "kimi",
  "dsh",
  "gemini",
  "grok",
  "qwen",
] as const;

export type QuotaSourceId = (typeof QUOTA_SOURCE_IDS)[number];

/** 额度区块里最常单独盯的来源，对应「常用」一键。 */
export const FAVORITE_QUOTA_SOURCES = ["codex", "claude", "cursor", "cursor_agent"] as const;

/** 官方额度区块可单独开关的账号，顺序与首页 / OfficialQuotaProvider::ALL 一致。 */
export const OFFICIAL_QUOTA_PROVIDER_IDS = [
  "claude",
  "codex",
  "cursor",
  "grok",
  "droid",
  "antigravity",
  "opencode",
  "copilot",
  "devin",
] as const;

export type OfficialQuotaProviderId = (typeof OFFICIAL_QUOTA_PROVIDER_IDS)[number];

export const OFFICIAL_QUOTA_PROVIDER_LABELS: Record<OfficialQuotaProviderId, string> = {
  claude: "Claude Code",
  codex: "Codex",
  cursor: "Cursor",
  grok: "Grok",
  droid: "Droid",
  antigravity: "Antigravity",
  opencode: "OpenCode",
  copilot: "Copilot",
  devin: "Devin",
};

export type OverviewLayout = {
  modules: Record<OverviewModuleId, boolean>;
  quotaSources: Record<string, boolean>;
  officialProviders: Record<string, boolean>;
};

export type OverviewLayoutSummary = {
  hiddenModules: OverviewModuleId[];
  hiddenPresentSources: string[];
  hiddenOfficialProviders: string[];
};

export function defaultOverviewLayout(): OverviewLayout {
  return {
    modules: {
      kpi: true,
      official: true,
      cursorAccount: true,
      billing: true,
      weekly: true,
      trend: true,
      heatmap: true,
      detail: true,
      status: true,
    },
    quotaSources: Object.fromEntries(QUOTA_SOURCE_IDS.map((id) => [id, true])),
    officialProviders: Object.fromEntries(OFFICIAL_QUOTA_PROVIDER_IDS.map((id) => [id, true])),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readFlag(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

export function parseOverviewLayout(raw: string | null): OverviewLayout {
  const defaults = defaultOverviewLayout();
  if (raw == null || raw === "") {
    return defaults;
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) {
      return defaults;
    }
    const modulesRaw = isRecord(parsed.modules) ? parsed.modules : {};
    const sourcesRaw = isRecord(parsed.quotaSources) ? parsed.quotaSources : {};
    const officialRaw = isRecord(parsed.officialProviders) ? parsed.officialProviders : {};
    const modules = { ...defaults.modules };
    for (const id of OVERVIEW_MODULE_IDS) {
      modules[id] = readFlag(modulesRaw[id], defaults.modules[id]);
    }
    const quotaSources = { ...defaults.quotaSources };
    for (const [source, visible] of Object.entries(sourcesRaw)) {
      if (typeof source === "string" && source.length > 0) {
        quotaSources[source] = readFlag(visible, true);
      }
    }
    const officialProviders = { ...defaults.officialProviders };
    for (const [provider, visible] of Object.entries(officialRaw)) {
      if (provider.length > 0) {
        officialProviders[provider] = readFlag(visible, true);
      }
    }
    return { modules, quotaSources, officialProviders };
  } catch {
    return defaults;
  }
}

export function readOverviewLayout(): OverviewLayout {
  try {
    return parseOverviewLayout(localStorage.getItem(OVERVIEW_LAYOUT_STORAGE_KEY));
  } catch {
    return defaultOverviewLayout();
  }
}

export function writeOverviewLayout(layout: OverviewLayout): void {
  try {
    localStorage.setItem(OVERVIEW_LAYOUT_STORAGE_KEY, JSON.stringify(layout));
  } catch {
    /* quota / private mode */
  }
}

export function isModuleVisible(layout: OverviewLayout, id: OverviewModuleId): boolean {
  return layout.modules[id] !== false;
}

export function isQuotaSourceVisible(layout: OverviewLayout, source: string): boolean {
  return layout.quotaSources[source] !== false;
}

export function filterQuotaItems<T extends { source: string }>(
  items: T[],
  layout: OverviewLayout,
): T[] {
  return items.filter((item) => isQuotaSourceVisible(layout, item.source));
}

export function isOfficialProviderVisible(layout: OverviewLayout, provider: string): boolean {
  // 自定义提供商的开关管的是「取不取数」，刻意不进「配置显示」。
  // localStorage 里即便被人写进了 custom: 的 false，首页也不得按这份配置藏它。
  if (isCustomQuotaProviderId(provider)) {
    return true;
  }
  return layout.officialProviders[provider] !== false;
}

export function filterOfficialQuotaRows<T extends { provider: string }>(
  rows: T[],
  layout: OverviewLayout,
): T[] {
  return rows.filter((row) => isOfficialProviderVisible(layout, row.provider));
}

/** 托盘额度面板按 official_quota.json 的 hidden_providers 过滤，和主窗口配置显示同步。 */
export function visibleOfficialQuotaRows<T extends { provider: string }>(
  rows: T[],
  hiddenProviders: string[] | undefined,
): T[] {
  if (!hiddenProviders || hiddenProviders.length === 0) {
    return rows;
  }
  const hidden = new Set(hiddenProviders);
  return rows.filter((row) => !hidden.has(row.provider));
}

export function isOfficialQuotaProviderId(value: string): value is OfficialQuotaProviderId {
  return (OFFICIAL_QUOTA_PROVIDER_IDS as readonly string[]).includes(value);
}

/** 自定义提供商的标识。它们不进「配置显示」——那一栏只列内置账号。 */
export function isCustomQuotaProviderId(provider: string): boolean {
  return provider.startsWith("custom:");
}

export function setModuleVisible(
  layout: OverviewLayout,
  id: OverviewModuleId,
  visible: boolean,
): OverviewLayout {
  return {
    ...layout,
    modules: { ...layout.modules, [id]: visible },
  };
}

export function setQuotaSourceVisible(
  layout: OverviewLayout,
  source: string,
  visible: boolean,
): OverviewLayout {
  return {
    ...layout,
    quotaSources: { ...layout.quotaSources, [source]: visible },
  };
}

export function setAllModulesVisible(layout: OverviewLayout, visible: boolean): OverviewLayout {
  const modules = { ...layout.modules };
  for (const id of OVERVIEW_MODULE_IDS) {
    modules[id] = visible;
  }
  return { ...layout, modules };
}

export function setAllQuotaSourcesVisible(
  layout: OverviewLayout,
  visible: boolean,
): OverviewLayout {
  const quotaSources = { ...layout.quotaSources };
  for (const id of QUOTA_SOURCE_IDS) {
    quotaSources[id] = visible;
  }
  for (const source of Object.keys(quotaSources)) {
    quotaSources[source] = visible;
  }
  return { ...layout, quotaSources };
}

export function setOfficialProviderVisible(
  layout: OverviewLayout,
  provider: string,
  visible: boolean,
): OverviewLayout {
  if (isCustomQuotaProviderId(provider)) {
    return layout;
  }
  return {
    ...layout,
    officialProviders: { ...layout.officialProviders, [provider]: visible },
  };
}

export function setAllOfficialProvidersVisible(
  layout: OverviewLayout,
  visible: boolean,
): OverviewLayout {
  const officialProviders = { ...layout.officialProviders };
  for (const id of OFFICIAL_QUOTA_PROVIDER_IDS) {
    officialProviders[id] = visible;
  }
  for (const provider of Object.keys(officialProviders)) {
    if (isCustomQuotaProviderId(provider)) {
      continue;
    }
    officialProviders[provider] = visible;
  }
  return { ...layout, officialProviders };
}

export function officialQuotaProviderLabel(provider: string): string {
  if (isOfficialQuotaProviderId(provider)) {
    return OFFICIAL_QUOTA_PROVIDER_LABELS[provider];
  }
  return provider;
}

export function visibleModuleCount(layout: OverviewLayout): number {
  return OVERVIEW_MODULE_IDS.filter((id) => isModuleVisible(layout, id)).length;
}

export function visibleQuotaSourceCount(layout: OverviewLayout): number {
  return QUOTA_SOURCE_IDS.filter((id) => isQuotaSourceVisible(layout, id)).length;
}

export function visibleOfficialProviderCount(layout: OverviewLayout): number {
  return OFFICIAL_QUOTA_PROVIDER_IDS.filter((id) => isOfficialProviderVisible(layout, id)).length;
}

export function applyQuotaSourceSet(
  layout: OverviewLayout,
  enabled: readonly string[],
): OverviewLayout {
  const allow = new Set(enabled);
  const quotaSources = { ...layout.quotaSources };
  for (const id of QUOTA_SOURCE_IDS) {
    quotaSources[id] = allow.has(id);
  }
  for (const source of Object.keys(quotaSources)) {
    quotaSources[source] = allow.has(source);
  }
  return { ...layout, quotaSources };
}

export function applyFavoriteQuotaSources(layout: OverviewLayout): OverviewLayout {
  return applyQuotaSourceSet(layout, FAVORITE_QUOTA_SOURCES);
}

export function applyDetectedQuotaSources(
  layout: OverviewLayout,
  detectedSources: readonly string[],
): OverviewLayout {
  return applyQuotaSourceSet(layout, detectedSources);
}

export function collectPresentSources(
  detectedSources: readonly string[],
  items: readonly { source: string }[],
): string[] {
  const present = new Set<string>();
  for (const source of detectedSources) {
    if (source) {
      present.add(source);
    }
  }
  for (const item of items) {
    if (item.source) {
      present.add(item.source);
    }
  }
  const known = QUOTA_SOURCE_IDS.filter((id) => present.has(id));
  const extra = [...present].filter(
    (source) => !QUOTA_SOURCE_IDS.includes(source as QuotaSourceId),
  );
  return [...known, ...extra];
}

export function quotaSourceChipIds(presentSources: readonly string[], showAll: boolean): string[] {
  if (showAll || presentSources.length === 0) {
    const extra = presentSources.filter(
      (source) => !QUOTA_SOURCE_IDS.includes(source as QuotaSourceId),
    );
    return [...QUOTA_SOURCE_IDS, ...extra];
  }
  return [...presentSources];
}

export function summarizeOverviewLayout(
  layout: OverviewLayout,
  presentSources: readonly string[] = [],
): OverviewLayoutSummary {
  const hiddenModules = OVERVIEW_MODULE_IDS.filter((id) => !isModuleVisible(layout, id));
  const sourcePool = presentSources.length > 0 ? presentSources : QUOTA_SOURCE_IDS;
  const hiddenPresentSources = sourcePool.filter((source) => !isQuotaSourceVisible(layout, source));
  const officialPool = new Set<string>(OFFICIAL_QUOTA_PROVIDER_IDS);
  for (const provider of Object.keys(layout.officialProviders)) {
    if (provider && !isCustomQuotaProviderId(provider)) {
      officialPool.add(provider);
    }
  }
  const hiddenOfficialProviders = [...officialPool].filter(
    (id) => !isOfficialProviderVisible(layout, id),
  );
  return { hiddenModules, hiddenPresentSources, hiddenOfficialProviders };
}
