import { DISPLAY_NAMES, OFFICIAL_QUOTA_PROVIDER_SLUGS } from "../generated/displayIdentity";

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
  billing: "本机估算窗口",
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
  "omp",
  "opencode",
  "kimi",
  "dsh",
  "gemini",
  "grok",
  "hermes",
  "agy",
  "qwen",
  "qoder",
  "qoder_cn",
  "cline",
  "roo",
  "kilo",
  "workbuddy",
  "codebuddy",
  "codebuddy_ide",
  "zcode",
  "alma",
  "trae",
  "goose",
  "amp",
  "zed",
  "cherry",
  "openclaw",
  "crush",
  "aider",
  "warp",
  "windsurf",
] as const;

export type QuotaSourceId = (typeof QUOTA_SOURCE_IDS)[number];

/** 额度区块里最常单独盯的来源，对应「常用」一键。 */
export const FAVORITE_QUOTA_SOURCES = ["codex", "claude", "cursor", "cursor_agent"] as const;

/** 官方额度区块可单独开关的账号，顺序与 `OfficialQuotaProvider::ALL` 一致。 */
export const OFFICIAL_QUOTA_PROVIDER_IDS = OFFICIAL_QUOTA_PROVIDER_SLUGS;

export type OfficialQuotaProviderId = (typeof OFFICIAL_QUOTA_PROVIDER_IDS)[number];

export type OverviewLayout = {
  modules: Record<OverviewModuleId, boolean>;
  quotaSources: Record<string, boolean>;
};

export type OverviewLayoutSummary = {
  hiddenModules: OverviewModuleId[];
  hiddenPresentSources: string[];
  hiddenOfficialProviders: string[];
};

export type LegacyOfficialProvidersMigration = {
  hasLegacyField: boolean;
  hiddenProviders: string[];
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
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readFlag(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function defaultBrowserStorage(): Storage | null {
  try {
    return typeof localStorage !== "undefined" ? localStorage : null;
  } catch {
    return null;
  }
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
    return { modules, quotaSources };
  } catch {
    return defaults;
  }
}

/**
 * 从旧版 `mabiao:overview-layout` 提取 `officialProviders` 中关闭的内置账号。
 * 忽略 `custom:*`；仅用于一次性迁移到 `official_quota.json`。
 */
export function extractLegacyHiddenOfficialProviders(
  raw: string | null,
): LegacyOfficialProvidersMigration {
  if (raw == null || raw === "") {
    return { hasLegacyField: false, hiddenProviders: [] };
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed) || !("officialProviders" in parsed)) {
      return { hasLegacyField: false, hiddenProviders: [] };
    }
    const officialRaw = isRecord(parsed.officialProviders) ? parsed.officialProviders : {};
    const seen = new Set<string>();
    const hiddenProviders: string[] = [];
    for (const id of OFFICIAL_QUOTA_PROVIDER_IDS) {
      if (officialRaw[id] === false && !seen.has(id)) {
        seen.add(id);
        hiddenProviders.push(id);
      }
    }
    for (const [provider, visible] of Object.entries(officialRaw)) {
      const trimmed = provider.trim();
      if (
        visible === false &&
        trimmed.length > 0 &&
        !isCustomQuotaProviderId(trimmed) &&
        !seen.has(trimmed)
      ) {
        seen.add(trimmed);
        hiddenProviders.push(trimmed);
      }
    }
    return { hasLegacyField: true, hiddenProviders };
  } catch {
    return { hasLegacyField: false, hiddenProviders: [] };
  }
}

/** 移除旧版 browser storage 中残留的 `officialProviders` 字段，保留 modules 与 quotaSources。 */
export function stripLegacyOfficialProvidersFromStorage(storage?: Storage | null): boolean {
  const target = storage === undefined ? defaultBrowserStorage() : storage;
  if (!target) {
    return false;
  }
  try {
    const raw = target.getItem(OVERVIEW_LAYOUT_STORAGE_KEY);
    if (raw == null || raw === "") {
      return false;
    }
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed) || !("officialProviders" in parsed)) {
      return false;
    }
    const next: Record<string, unknown> = { ...parsed };
    delete next.officialProviders;
    target.setItem(OVERVIEW_LAYOUT_STORAGE_KEY, JSON.stringify(next));
    return true;
  } catch {
    return false;
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
    localStorage.setItem(
      OVERVIEW_LAYOUT_STORAGE_KEY,
      JSON.stringify({ modules: layout.modules, quotaSources: layout.quotaSources }),
    );
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

/** 清理 hidden_providers：去空白、丢弃 `custom:*`、保序去重。 */
export function sanitizeHiddenOfficialProviders(
  hiddenProviders: readonly string[] | undefined,
): string[] {
  if (!hiddenProviders || hiddenProviders.length === 0) {
    return [];
  }
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of hiddenProviders) {
    const trimmed = raw.trim();
    if (!trimmed || isCustomQuotaProviderId(trimmed) || seen.has(trimmed)) {
      continue;
    }
    seen.add(trimmed);
    out.push(trimmed);
  }
  return out;
}

export function isOfficialProviderVisible(
  hiddenProviders: readonly string[] | undefined,
  provider: string,
): boolean {
  // 自定义提供商的开关管的是「取不取数」，刻意不进「配置显示」。
  // 哪怕配置里出现 custom:，主窗口和托盘都不得藏它。
  if (isCustomQuotaProviderId(provider)) {
    return true;
  }
  if (!hiddenProviders || hiddenProviders.length === 0) {
    return true;
  }
  const key = acctProviderSlug(provider) ?? provider;
  return !hiddenProviders.includes(key);
}

/** 主窗口与托盘统一按 `official_quota.json` 的 `hidden_providers` 过滤官方额度行。 */
export function visibleOfficialQuotaRows<
  T extends { provider: string; base_provider?: string | null },
>(rows: T[], hiddenProviders: readonly string[] | undefined): T[] {
  if (!hiddenProviders || hiddenProviders.length === 0) {
    return rows;
  }
  return rows.filter((row) =>
    isOfficialProviderVisible(hiddenProviders, officialQuotaAttributionId(row)),
  );
}

export function visibleUndetectedOfficialProviders(
  undetected: readonly string[],
  hiddenProviders: readonly string[] | undefined,
): string[] {
  if (!hiddenProviders || hiddenProviders.length === 0) {
    return [...undetected];
  }
  return undetected.filter((id) => isOfficialProviderVisible(hiddenProviders, id));
}

/** `acct:<slug>:<hex>` 中间那段内置 slug。不是这个形状就返回 null。 */
export function acctProviderSlug(provider: string): string | null {
  const match = /^acct:([a-z0-9_-]+):([0-9a-fA-F]+)$/.exec(provider);
  return match?.[1] ?? null;
}

/**
 * 配置显示、图标和 Copilot 重置文案用的提供方。
 * 自定义提供商保持自己的 `custom:` 标识；额度账号用 `base_provider`，没有再从 id 里取。
 */
export function officialQuotaAttributionId(row: {
  provider: string;
  base_provider?: string | null;
}): string {
  if (isCustomQuotaProviderId(row.provider)) {
    return row.provider;
  }
  const base = row.base_provider?.trim();
  if (base) {
    return base;
  }
  return acctProviderSlug(row.provider) ?? row.provider;
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
  hiddenProviders: readonly string[] | undefined,
  provider: string,
  visible: boolean,
): string[] {
  const current = sanitizeHiddenOfficialProviders(hiddenProviders);
  if (isCustomQuotaProviderId(provider)) {
    return current;
  }
  const key = (acctProviderSlug(provider) ?? provider).trim();
  if (!key) {
    return current;
  }
  if (visible) {
    return current.filter((id) => id !== key);
  }
  if (current.includes(key)) {
    return current;
  }
  return [...current, key];
}

export function setAllOfficialProvidersVisible(visible: boolean): string[] {
  return visible ? [] : [...OFFICIAL_QUOTA_PROVIDER_IDS];
}

export function officialQuotaProviderLabel(provider: string): string {
  if (isOfficialQuotaProviderId(provider)) {
    return DISPLAY_NAMES[provider];
  }
  return provider;
}

export function visibleModuleCount(layout: OverviewLayout): number {
  return OVERVIEW_MODULE_IDS.filter((id) => isModuleVisible(layout, id)).length;
}

export function visibleQuotaSourceCount(layout: OverviewLayout): number {
  return QUOTA_SOURCE_IDS.filter((id) => isQuotaSourceVisible(layout, id)).length;
}

export function visibleOfficialProviderCount(
  hiddenProviders: readonly string[] | undefined,
): number {
  return OFFICIAL_QUOTA_PROVIDER_IDS.filter((id) =>
    isOfficialProviderVisible(hiddenProviders, id),
  ).length;
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
  hiddenOfficialProviders: readonly string[] = [],
): OverviewLayoutSummary {
  const hiddenModules = OVERVIEW_MODULE_IDS.filter((id) => !isModuleVisible(layout, id));
  const sourcePool = presentSources.length > 0 ? presentSources : QUOTA_SOURCE_IDS;
  const hiddenPresentSources = sourcePool.filter((source) => !isQuotaSourceVisible(layout, source));
  const sanitizedHidden = sanitizeHiddenOfficialProviders(hiddenOfficialProviders);
  const hiddenSet = new Set(sanitizedHidden);
  const knownHidden = OFFICIAL_QUOTA_PROVIDER_IDS.filter((id) => hiddenSet.has(id));
  const extraHidden = sanitizedHidden.filter(
    (id) => !(OFFICIAL_QUOTA_PROVIDER_IDS as readonly string[]).includes(id),
  );
  return {
    hiddenModules,
    hiddenPresentSources,
    hiddenOfficialProviders: [...knownHidden, ...extraHidden],
  };
}
