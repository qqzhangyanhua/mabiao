import { heatmapFilter } from "../../lib/calendar";
import { customRangeFilter, humanStatus, previousFilter, rangeFromPreset } from "../../lib/format";
import { ingestFailureStatus, usageStageStatus } from "../../lib/ingestStatus";
import {
  popRangeHistory,
  pushRangeHistory,
  rangeSnapshot,
  sameRange,
  type RangeSnapshot,
} from "../../lib/rangeHistory";
import { views } from "../../lib/viewHash";
import type {
  ApplicationAnalyticsDto,
  BillingWindowsDto,
  BudgetConfig,
  BudgetStatusDto,
  CodeVolumeSummary,
  ConversationFocus,
  CursorAccountUsageDto,
  CursorSessionSummaryDto,
  Filter,
  FilterOptions,
  Grain,
  IngestIssue,
  IngestReport,
  NamedAmount,
  OfficialQuotaDto,
  OverviewDto,
  PriceTable,
  SeriesPoint,
  SessionRow,
  SourceDiagnostic,
  View,
} from "../../types";
import { emptyFilter } from "./constants";
import type { UsageBackend } from "./usageBackend";

export type ViewScope = {
  filter: Filter;
  preset: string;
};

export type FreshnessRevisions = {
  data: number;
  pricing: number;
  cursor: number;
};

type ViewDependency = {
  grain: boolean;
  pricing: boolean;
  cursor: boolean;
};

type ViewFreshnessEntry = {
  scope: ViewScope;
  grain: Grain;
  revisions: FreshnessRevisions;
};

const DEFAULT_RANGE_PRESET = "7";
const OVERVIEW_TOP_SESSIONS_LIMIT = 8;

const LOCAL_ONLY_VIEWS: ReadonlySet<View> = new Set([
  "conversations",
  "work-notes",
  "cursor",
  "cursor-sessions",
  "worktime",
  "instructions",
  "settings",
]);

const CURSOR_OVERLAY_VIEWS: readonly View[] = ["overview", "trend", "application", "project"];
const CURSOR_OVERLAY_VIEW_SET: ReadonlySet<View> = new Set(CURSOR_OVERLAY_VIEWS);

const VIEW_DEPENDENCIES: Record<View, ViewDependency> = {
  overview: { grain: true, pricing: true, cursor: true },
  trend: { grain: true, pricing: true, cursor: true },
  application: { grain: true, pricing: false, cursor: true },
  model: { grain: false, pricing: true, cursor: false },
  provider: { grain: false, pricing: true, cursor: false },
  project: { grain: false, pricing: true, cursor: true },
  conversations: { grain: false, pricing: false, cursor: false },
  "work-notes": { grain: false, pricing: false, cursor: false },
  cursor: { grain: false, pricing: true, cursor: true },
  "cursor-sessions": { grain: false, pricing: false, cursor: false },
  worktime: { grain: false, pricing: false, cursor: false },
  instructions: { grain: false, pricing: false, cursor: false },
  settings: { grain: false, pricing: true, cursor: false },
};

function sameItems(left: string[], right: string[]): boolean {
  if (left.length !== right.length) {
    return false;
  }
  const other = new Set(right);
  return left.every((item) => other.has(item));
}

export function filtersEqual(left: Filter, right: Filter): boolean {
  return (
    left.from === right.from &&
    left.to === right.to &&
    sameItems(left.sources, right.sources) &&
    sameItems(left.models, right.models) &&
    sameItems(left.projects, right.projects) &&
    sameItems(left.providers, right.providers)
  );
}

export function scopesEqual(left: ViewScope, right: ViewScope): boolean {
  return left.preset === right.preset && filtersEqual(left.filter, right.filter);
}

export function emptyViewScope(preset = DEFAULT_RANGE_PRESET): ViewScope {
  return {
    filter: {
      ...emptyFilter,
      ...rangeFromPreset(preset),
      sources: [],
      models: [],
      projects: [],
      providers: [],
    },
    preset,
  };
}

export function initialViewScopes(preset = DEFAULT_RANGE_PRESET): Record<View, ViewScope> {
  const seed = emptyViewScope(preset);
  const scopes = {} as Record<View, ViewScope>;
  for (const view of views) {
    scopes[view] = {
      preset: seed.preset,
      filter: {
        ...seed.filter,
        sources: [],
        models: [],
        projects: [],
        providers: [],
      },
    };
  }
  return scopes;
}

/** Topbar 筛选跨页共用。用量来源不覆盖对话记录来源：Cursor Agent 往往没有消耗记录。 */
export function syncSharedFilters(
  scopes: Record<View, ViewScope>,
  filter: Filter,
  preset: string,
  origin: View,
): Record<View, ViewScope> {
  let changed = false;
  const next = { ...scopes };
  for (const view of views) {
    const scope = next[view];
    const nextSources =
      view === "conversations"
        ? origin === "conversations"
          ? filter.sources
          : scope.filter.sources
        : origin === "conversations"
          ? scope.filter.sources
          : filter.sources;
    const nextScope: ViewScope = {
      preset,
      filter: { ...filter, sources: nextSources },
    };
    if (scopesEqual(scope, nextScope)) {
      continue;
    }
    changed = true;
    next[view] = nextScope;
  }
  return changed ? next : scopes;
}

/** 拉概览时已经带上了趋势、模型、项目，切到这些页不应再打一轮查询。 */
export function viewsWarmedBy(view: View): View[] {
  if (view === "overview") {
    return ["overview", "trend", "model", "project"];
  }
  return [view];
}

/** 本次查询会覆盖这些视图正在展示的共享数据集。 */
export function viewsInvalidatedBy(view: View): View[] {
  switch (view) {
    case "overview":
      return ["trend", "model", "project"];
    case "trend":
    case "model":
    case "project":
      return ["overview"];
    default:
      return [];
  }
}

function isEntryFresh(
  entry: ViewFreshnessEntry | undefined,
  view: View,
  scope: ViewScope,
  grain: Grain,
  revisions: FreshnessRevisions,
): boolean {
  if (!entry) {
    return false;
  }
  if (entry.revisions.data !== revisions.data) {
    return false;
  }
  if (!scopesEqual(entry.scope, scope)) {
    return false;
  }
  const deps = VIEW_DEPENDENCIES[view];
  if (deps.grain && entry.grain !== grain) {
    return false;
  }
  if (deps.pricing && entry.revisions.pricing !== revisions.pricing) {
    return false;
  }
  if (deps.cursor && entry.revisions.cursor !== revisions.cursor) {
    return false;
  }
  return true;
}

function isEntryFreshIgnoringGrain(
  entry: ViewFreshnessEntry | undefined,
  view: View,
  scope: ViewScope,
  revisions: FreshnessRevisions,
): boolean {
  if (!entry) {
    return false;
  }
  if (entry.revisions.data !== revisions.data) {
    return false;
  }
  if (!scopesEqual(entry.scope, scope)) {
    return false;
  }
  const deps = VIEW_DEPENDENCIES[view];
  if (deps.pricing && entry.revisions.pricing !== revisions.pricing) {
    return false;
  }
  if (deps.cursor && entry.revisions.cursor !== revisions.cursor) {
    return false;
  }
  return true;
}

type StateUpdater<T> = T | ((prev: T) => T);

function resolveUpdater<T>(current: T, next: StateUpdater<T>): T {
  return typeof next === "function" ? (next as (prev: T) => T)(current) : next;
}

export type UsageControllerSnapshot = {
  view: View;
  filter: Filter;
  preset: string;
  grain: Grain;
  options: FilterOptions;
  overview: OverviewDto | null;
  billingWindows: BillingWindowsDto | null;
  officialQuota: OfficialQuotaDto | null;
  cursorAccountUsage: CursorAccountUsageDto | null;
  previous: OverviewDto | null;
  trend: SeriesPoint[];
  heatmap: SeriesPoint[];
  heatmapRange: { from: string; to: string };
  applicationAnalytics: ApplicationAnalyticsDto | null;
  models: NamedAmount[];
  projects: NamedAmount[];
  providerBreakdown: NamedAmount[];
  breakdown: NamedAmount[];
  sessions: SessionRow[];
  prices: PriceTable;
  budgetStatus: BudgetStatusDto | null;
  diagnostics: SourceDiagnostic[];
  codeVolume: CodeVolumeSummary | null;
  codeVolumeLoading: boolean;
  cursorSessionSummary: CursorSessionSummaryDto | null;
  cursorSessionLoading: boolean;
  loading: boolean;
  updatedAt: string | null;
  viewHasData: boolean;
  sessionsRevision: number;
  pricingRevision: number;
  cursorAccountRevision: number;
  restoreRevision: number;
  cursorAccountRefreshError: string | null;
  conversationFocus: ConversationFocus | null;
  savingBudget: boolean;
  lastIngestReport: IngestReport | null;
  conversationIssues: IngestIssue[];
  busy: boolean;
  rebuilding: string | null;
  purging: string | null;
  status: string;
  connected: boolean;
  canGoBack: boolean;
};

type InternalState = Omit<
  UsageControllerSnapshot,
  "filter" | "preset" | "breakdown" | "viewHasData"
>;

export type UsageControllerOptions = {
  backend: UsageBackend;
  initialView?: View;
  initialPreset?: string;
  initialConversationFocus?: ConversationFocus | null;
  onClearSessionDetailCache?: () => void;
  onBeginUsageStage?: () => void;
  onPublishUsageStatus?: (text: string) => void;
};

function deriveSnapshot(
  state: InternalState,
  viewScopes: Record<View, ViewScope>,
  hydratedViews: ReadonlySet<View>,
): UsageControllerSnapshot {
  const { filter, preset } = viewScopes[state.view];
  const breakdown =
    state.view === "provider"
      ? state.providerBreakdown
      : state.view === "project"
        ? state.projects
        : state.models;
  return {
    ...state,
    filter,
    preset,
    breakdown,
    viewHasData: hydratedViews.has(state.view),
  };
}

export class UsageViewController {
  private readonly backend: UsageBackend;
  private readonly onClearSessionDetailCache: () => void;
  private onBeginUsageStage: () => void;
  private onPublishUsageStatus: (text: string) => void;
  private readonly listeners = new Set<() => void>();

  private viewScopes: Record<View, ViewScope>;
  private rangeHistory: RangeSnapshot[] = [];
  private hydratedViews = new Set<View>();
  private freshness: Partial<Record<View, ViewFreshnessEntry>> = {};
  private revisions: FreshnessRevisions = { data: 0, pricing: 0, cursor: 0 };
  private optionsEpoch = -1;
  private requestGeneration = 0;
  private wideRefreshGeneration: number | null = null;
  private cursorRefreshInflight: Promise<void> | null = null;
  private ingestOperationInFlight = false;

  private state: InternalState;
  private snapshot: UsageControllerSnapshot;

  constructor(options: UsageControllerOptions) {
    this.backend = options.backend;
    this.onClearSessionDetailCache = options.onClearSessionDetailCache ?? (() => undefined);
    this.onBeginUsageStage = options.onBeginUsageStage ?? (() => undefined);
    this.onPublishUsageStatus =
      options.onPublishUsageStatus ??
      ((text: string) => {
        this.patch({ status: text });
      });
    const initialView = options.initialView ?? "overview";
    this.viewScopes = initialViewScopes(options.initialPreset ?? DEFAULT_RANGE_PRESET);
    const heat = heatmapFilter(emptyFilter);
    this.state = {
      view: initialView,
      grain: "day",
      options: { sources: [], models: [], projects: [], providers: [] },
      overview: null,
      billingWindows: null,
      officialQuota: null,
      cursorAccountUsage: null,
      previous: null,
      trend: [],
      heatmap: [],
      heatmapRange: { from: heat.fromDate, to: heat.toDate },
      applicationAnalytics: null,
      models: [],
      projects: [],
      providerBreakdown: [],
      sessions: [],
      prices: { prices: [] },
      budgetStatus: null,
      diagnostics: [],
      codeVolume: null,
      codeVolumeLoading: initialView === "cursor",
      cursorSessionSummary: null,
      cursorSessionLoading: initialView === "cursor-sessions",
      loading: !LOCAL_ONLY_VIEWS.has(initialView),
      updatedAt: null,
      sessionsRevision: 0,
      pricingRevision: 0,
      cursorAccountRevision: 0,
      restoreRevision: 0,
      cursorAccountRefreshError: null,
      conversationFocus: options.initialConversationFocus ?? null,
      savingBudget: false,
      lastIngestReport: null,
      conversationIssues: [],
      busy: false,
      rebuilding: null,
      purging: null,
      status: "正在连接…",
      connected: false,
      canGoBack: false,
    };
    this.snapshot = deriveSnapshot(this.state, this.viewScopes, this.hydratedViews);
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  getSnapshot = (): UsageControllerSnapshot => this.snapshot;

  private emit(): void {
    this.snapshot = deriveSnapshot(this.state, this.viewScopes, this.hydratedViews);
    for (const listener of this.listeners) {
      listener();
    }
  }

  private patch(update: Partial<InternalState>): void {
    this.state = { ...this.state, ...update };
    this.emit();
  }

  reportError = (error: unknown): void => {
    this.patch({ status: humanStatus(error) });
  };

  setCatalogStatusHandlers = (handlers: {
    beginUsageStage: () => void;
    publishUsageStatus: (text: string) => void;
  }): void => {
    this.onBeginUsageStage = handlers.beginUsageStage;
    this.onPublishUsageStatus = handlers.publishUsageStatus;
  };

  setStatus = (next: StateUpdater<string>): void => {
    this.patch({ status: resolveUpdater(this.state.status, next) });
  };

  setPrices = (next: StateUpdater<PriceTable>): void => {
    this.patch({ prices: resolveUpdater(this.state.prices, next) });
  };

  setOfficialQuota = (next: StateUpdater<OfficialQuotaDto | null>): void => {
    this.patch({ officialQuota: resolveUpdater(this.state.officialQuota, next) });
  };

  setConversationIssues = (issues: IngestIssue[]): void => {
    this.patch({ conversationIssues: issues });
  };

  bumpSessionsRevision = (): void => {
    this.patch({ sessionsRevision: this.state.sessionsRevision + 1 });
  };

  setConversationFocus = (focus: ConversationFocus | null): void => {
    this.patch({ conversationFocus: focus });
  };

  clearConversationFocus = (): void => {
    this.patch({ conversationFocus: null });
  };

  connectAndStart = async (): Promise<void> => {
    try {
      await this.backend.ping();
      this.patch({ connected: true, status: "正在加载缓存…" });
      try {
        await this.refreshActiveView();
      } catch (error: unknown) {
        this.reportError(error);
        this.patch({ loading: false });
      }
      await this.runIngest("启动摄取");
    } catch (error: unknown) {
      this.patch({
        connected: false,
        status: humanStatus(error),
        loading: false,
      });
    }
  };

  setGrain = async (next: Grain): Promise<void> => {
    if (next === this.state.grain) {
      return;
    }
    this.patch({ grain: next });
    await this.refreshTrend();
  };

  private async applyScope(
    nextPreset: string,
    explicitRange?: { from: string | null; to: string | null },
  ): Promise<void> {
    const view = this.state.view;
    const currentFilter = this.viewScopes[view].filter;
    const range = explicitRange ?? rangeFromPreset(nextPreset);
    const nextFilter: Filter = { ...currentFilter, ...range };
    this.viewScopes = syncSharedFilters(this.viewScopes, nextFilter, nextPreset, view);
    this.emit();
    await this.refreshActiveView(nextFilter, nextPreset);
  }

  applyPreset = async (
    next: string,
    explicitRange?: { from: string | null; to: string | null },
  ): Promise<void> => {
    if (this.rangeHistory.length > 0) {
      this.rangeHistory = [];
      this.patch({ canGoBack: false });
    }
    await this.applyScope(next, explicitRange);
  };

  drillRange = async (from: string, to: string): Promise<void> => {
    const scope = this.viewScopes[this.state.view];
    const range = customRangeFilter(from, to);
    const current = rangeSnapshot(scope.preset, scope.filter.from, scope.filter.to);
    const next = rangeSnapshot("custom", range.from, range.to);
    if (sameRange(current, next)) {
      return;
    }
    this.rangeHistory = pushRangeHistory(this.rangeHistory, current, next);
    this.patch({ canGoBack: this.rangeHistory.length > 0 });
    await this.applyScope("custom", range);
  };

  popRange = async (): Promise<void> => {
    const popped = popRangeHistory(this.rangeHistory);
    if (!popped.previous) {
      return;
    }
    this.rangeHistory = popped.history;
    this.patch({ canGoBack: this.rangeHistory.length > 0 });
    await this.applyScope(popped.previous.preset, {
      from: popped.previous.from,
      to: popped.previous.to,
    });
  };

  runIngest = async (label: string): Promise<void> => {
    if (this.ingestOperationInFlight) {
      return;
    }
    this.onClearSessionDetailCache();
    this.ingestOperationInFlight = true;
    this.requestGeneration += 1;
    this.patch({ busy: true, status: `${label}中…` });
    this.onBeginUsageStage();
    try {
      const report = await this.backend.ingest();
      this.revisions = { ...this.revisions, data: this.revisions.data + 1 };
      this.patch({
        sessionsRevision: this.state.sessionsRevision + 1,
        lastIngestReport: report,
      });
      this.onPublishUsageStatus(usageStageStatus(label, report));
      await this.refreshActiveView();
      try {
        await this.backend.refreshTray();
      } catch {
        // 菜单栏刷新失败不阻断主界面
      }
    } catch (error) {
      this.patch({
        status: ingestFailureStatus(label, humanStatus(error)),
        loading: false,
      });
    } finally {
      this.ingestOperationInFlight = false;
      this.patch({ busy: false });
    }
  };

  runRebuild = async (source: string | null): Promise<void> => {
    if (this.ingestOperationInFlight) {
      return;
    }
    this.onClearSessionDetailCache();
    this.ingestOperationInFlight = true;
    this.requestGeneration += 1;
    const target = source ?? "all";
    this.patch({
      rebuilding: target,
      busy: true,
      status: `${source ? `${source} ` : "全部"}缓存重建中…`,
    });
    try {
      const report = await this.backend.rebuildCache(source);
      this.revisions = { ...this.revisions, data: this.revisions.data + 1 };
      const archived = report.records_archived > 0 ? `，归档 ${report.records_archived}` : "";
      this.patch({
        sessionsRevision: this.state.sessionsRevision + 1,
        lastIngestReport: report,
        conversationIssues: report.conversation_issues,
        status: `缓存重建${report.partial_success ? "部分完成" : "完成"}：写入 ${report.records_written}${archived}，清理 ${report.records_removed}，失败 ${report.files_failed}`,
      });
      await this.refreshActiveView();
      try {
        await this.backend.refreshTray();
      } catch {
        // 菜单栏刷新失败不阻断主界面
      }
    } catch (error) {
      this.patch({
        status: `缓存重建失败：${humanStatus(error)}`,
        loading: false,
      });
    } finally {
      this.ingestOperationInFlight = false;
      this.patch({ rebuilding: null, busy: false });
    }
  };

  runPurgeArchived = async (source: string | null): Promise<void> => {
    if (this.ingestOperationInFlight) {
      return;
    }
    this.ingestOperationInFlight = true;
    const target = source ?? "all";
    this.patch({
      purging: target,
      busy: true,
      status: `正在清理${source ? `${source} ` : "全部"}已归档记录…`,
    });
    try {
      const removed = await this.backend.purgeArchivedRecords(source);
      this.revisions = { ...this.revisions, data: this.revisions.data + 1 };
      this.patch({
        sessionsRevision: this.state.sessionsRevision + 1,
        status: `已永久删除 ${removed} 条归档记录`,
      });
      await this.refreshActiveView();
    } catch (error) {
      this.patch({ status: `清理归档记录失败：${humanStatus(error)}` });
    } finally {
      this.ingestOperationInFlight = false;
      this.patch({ purging: null, busy: false });
    }
  };

  refreshTrend = async (overrideFilter?: Filter): Promise<void> => {
    const view = this.state.view;
    if (view !== "overview" && view !== "trend" && view !== "application") {
      return;
    }
    const activeScope = this.viewScopes[view];
    const nextFilter = overrideFilter ?? activeScope.filter;
    const usedScope: ViewScope = { filter: nextFilter, preset: activeScope.preset };
    const grain = this.state.grain;

    // 宽刷新在飞时升级，避免窄刷新取消它之后只补回趋势。
    if (this.wideRefreshGeneration !== null) {
      return this.refreshActiveView(nextFilter, activeScope.preset);
    }

    const capturedRevisions: FreshnessRevisions = { ...this.revisions };
    const overviewStampedBefore = isEntryFreshIgnoringGrain(
      this.freshness.overview,
      "overview",
      usedScope,
      capturedRevisions,
    );
    const generation = ++this.requestGeneration;
    const commit = (update: Partial<InternalState>) => {
      if (generation === this.requestGeneration) {
        this.patch(update);
      }
    };

    const tasks: Array<Promise<void>> = [];
    if (view === "overview" || view === "trend") {
      tasks.push(
        this.backend.getTrend(nextFilter, grain).then((trend) => {
          commit({ trend });
        }),
      );
    }
    if (view === "application") {
      tasks.push(
        this.backend.getApplicationAnalytics(nextFilter, grain).then((applicationAnalytics) => {
          commit({ applicationAnalytics });
        }),
      );
    }

    this.patch({ loading: true });
    try {
      await Promise.all(tasks);
    } finally {
      if (generation === this.requestGeneration) {
        this.patch({ loading: false });
        if (view === "overview" || view === "trend") {
          this.markHydrated("trend", usedScope, grain, capturedRevisions);
          if (overviewStampedBefore) {
            this.freshness = {
              ...this.freshness,
              overview: {
                scope: {
                  preset: usedScope.preset,
                  filter: {
                    ...usedScope.filter,
                    sources: [...usedScope.filter.sources],
                    models: [...usedScope.filter.models],
                    projects: [...usedScope.filter.projects],
                    providers: [...usedScope.filter.providers],
                  },
                },
                grain,
                revisions: { ...capturedRevisions },
              },
            };
          }
        }
        if (view === "application") {
          this.markHydrated("application", usedScope, grain, capturedRevisions);
        }
      }
    }
    if (generation === this.requestGeneration) {
      this.patch({ updatedAt: new Date().toISOString() });
    }
  };

  private bumpPricingRevision(): void {
    const nextPricing = this.revisions.pricing + 1;
    this.revisions = { ...this.revisions, pricing: nextPricing };
    this.patch({ pricingRevision: nextPricing });
  }

  private async refreshPricingActiveView(): Promise<void> {
    if (!VIEW_DEPENDENCIES[this.state.view].pricing) {
      return;
    }
    if (this.state.view === "settings") {
      const generation = ++this.requestGeneration;
      const capturedRevisions: FreshnessRevisions = { ...this.revisions };
      const activeScope = this.viewScopes.settings;
      const budgetStatus = await this.backend.getBudgetStatus();
      if (generation === this.requestGeneration) {
        this.patch({ budgetStatus });
        if (
          this.freshness.settings &&
          this.freshness.settings.revisions.data === capturedRevisions.data &&
          scopesEqual(this.freshness.settings.scope, activeScope)
        ) {
          this.freshness = {
            ...this.freshness,
            settings: {
              ...this.freshness.settings,
              revisions: { ...capturedRevisions },
            },
          };
        }
      }
      return;
    }
    await this.refreshActiveView();
  }

  savePrices = async (prices?: PriceTable): Promise<void> => {
    const nextPrices = prices ?? this.state.prices;
    try {
      await this.backend.savePriceTable(nextPrices);
      this.patch({ prices: nextPrices, status: "单价已保存" });
      this.bumpPricingRevision();
      await this.refreshPricingActiveView();
    } catch (error) {
      this.reportError(error);
      throw error;
    }
  };

  invalidatePricing = async (): Promise<void> => {
    this.bumpPricingRevision();
    await this.refreshPricingActiveView();
  };

  saveBudget = async (config: BudgetConfig): Promise<void> => {
    this.patch({ savingBudget: true });
    try {
      await this.backend.saveBudget(config);
      this.bumpPricingRevision();
      await this.refreshPricingActiveView();
      this.patch({ status: "预算设置已保存" });
    } catch (error) {
      this.patch({ status: `预算设置保存失败：${humanStatus(error)}` });
      throw error;
    } finally {
      this.patch({ savingBudget: false });
    }
  };

  /**
   * 备份恢复后的专用全量重载：不触发磁盘摄取，重新加载恢复后的 SQLite 数据、单价、
   * 月度预算、诊断/扫描路径与官方额度，并使全部视图缓存失效。
   */
  reloadAfterRestore = async (): Promise<void> => {
    this.onClearSessionDetailCache();
    this.revisions = {
      data: this.revisions.data + 1,
      pricing: this.revisions.pricing + 1,
      cursor: this.revisions.cursor + 1,
    };
    this.optionsEpoch = -1;
    this.freshness = {};
    this.hydratedViews = new Set();
    const generation = ++this.requestGeneration;
    this.wideRefreshGeneration = null;
    const capturedRevisions: FreshnessRevisions = { ...this.revisions };
    const activeView = this.state.view;
    const activeScope = this.viewScopes[activeView];

    this.patch({
      sessionsRevision: this.state.sessionsRevision + 1,
      pricingRevision: this.revisions.pricing,
      cursorAccountRevision: this.revisions.cursor,
      restoreRevision: this.state.restoreRevision + 1,
      cursorAccountRefreshError: null,
      lastIngestReport: null,
      overview: null,
      previous: null,
      billingWindows: null,
      cursorAccountUsage: null,
      trend: [],
      heatmap: [],
      applicationAnalytics: null,
      models: [],
      projects: [],
      providerBreakdown: [],
      sessions: [],
      codeVolume: null,
      cursorSessionSummary: null,
      status: "正在加载恢复的备份…",
    });

    try {
      const [
        options,
        prices,
        diagnostics,
        budgetStatus,
        officialQuota,
        conversationIssues,
        cursorAccountUsage,
      ] = await Promise.all([
        this.backend.getFilterOptions(),
        this.backend.getPrices(),
        this.backend.getSourceDiagnostics(),
        this.backend.getBudgetStatus(),
        this.backend.getOfficialQuota(),
        this.backend.getConversationCatalogIssues(),
        this.backend.getCursorAccountUsage(activeScope.filter),
      ]);
      if (generation !== this.requestGeneration) {
        return;
      }
      this.optionsEpoch = capturedRevisions.data;
      this.patch({
        options,
        prices,
        diagnostics,
        budgetStatus,
        officialQuota,
        conversationIssues,
        cursorAccountUsage,
        status: "已恢复备份",
        updatedAt: new Date().toISOString(),
      });
      if (activeView === "settings") {
        this.markHydrated("settings", activeScope, this.state.grain, capturedRevisions);
      } else {
        await this.refreshActiveView();
      }
    } catch (error) {
      this.reportError(error);
      throw error;
    }
  };

  private markHydrated(
    target: View,
    used: ViewScope,
    grain: Grain,
    capturedRevisions: FreshnessRevisions,
  ): void {
    const nextFreshness = { ...this.freshness };
    const nextHydrated = new Set(this.hydratedViews);
    nextHydrated.add(target);
    for (const warmed of viewsWarmedBy(target)) {
      const scope = warmed === target ? used : this.viewScopes[warmed];
      if (scopesEqual(scope, used)) {
        nextFreshness[warmed] = {
          scope: {
            preset: scope.preset,
            filter: {
              ...scope.filter,
              sources: [...scope.filter.sources],
              models: [...scope.filter.models],
              projects: [...scope.filter.projects],
              providers: [...scope.filter.providers],
            },
          },
          grain,
          revisions: { ...capturedRevisions },
        };
        nextHydrated.add(warmed);
      }
    }
    for (const invalidated of viewsInvalidatedBy(target)) {
      if (!scopesEqual(this.viewScopes[invalidated], used)) {
        delete nextFreshness[invalidated];
      }
    }
    this.freshness = nextFreshness;
    this.hydratedViews = nextHydrated;
    this.emit();
  }

  navigate = async (next: View): Promise<void> => {
    this.patch({ view: next });
    const scope = this.viewScopes[next];
    if (isEntryFresh(this.freshness[next], next, scope, this.state.grain, this.revisions)) {
      return;
    }
    await this.refreshActiveView();
  };

  applyViewFilter = async (target: View, next: Filter): Promise<void> => {
    this.viewScopes = syncSharedFilters(
      this.viewScopes,
      next,
      this.viewScopes[target].preset,
      target,
    );
    this.emit();
    if (target === this.state.view) {
      await this.refreshActiveView(next);
    }
  };

  applyFilter = async (next: Filter): Promise<void> => {
    await this.applyViewFilter(this.state.view, next);
  };

  clearCursorAccountUsage = async (): Promise<void> => {
    const cleared = await this.backend.clearCursorAccountUsage();
    const nextCursorRevision = this.revisions.cursor + 1;
    this.revisions = { ...this.revisions, cursor: nextCursorRevision };
    // 取消此前在飞的视图查询，避免清空前查出的旧 Cursor 行回写成幽灵数据。
    this.requestGeneration += 1;
    this.wideRefreshGeneration = null;

    const activeView = this.state.view;
    const nextFreshness = { ...this.freshness };
    const nextHydrated = new Set(this.hydratedViews);
    for (const view of [...CURSOR_OVERLAY_VIEWS, "cursor" as const]) {
      delete nextFreshness[view];
      if (view !== activeView) {
        nextHydrated.delete(view);
      }
    }
    this.freshness = nextFreshness;
    this.hydratedViews = nextHydrated;

    this.patch({
      cursorAccountRevision: nextCursorRevision,
      cursorAccountRefreshError: null,
      cursorAccountUsage: cleared,
      ...(CURSOR_OVERLAY_VIEW_SET.has(activeView)
        ? {}
        : {
            billingWindows: null,
            trend: [],
            applicationAnalytics: null,
            projects: [],
          }),
    });

    if (CURSOR_OVERLAY_VIEW_SET.has(activeView)) {
      await this.refreshActiveView();
    }
  };

  refreshCursorAccount = async (): Promise<void> => {
    if (this.cursorRefreshInflight) {
      return this.cursorRefreshInflight;
    }
    const run = (async () => {
      try {
        await this.backend.refreshCursorAccountUsage();
        const nextCursorRevision = this.revisions.cursor + 1;
        this.revisions = { ...this.revisions, cursor: nextCursorRevision };
        this.patch({
          cursorAccountRevision: nextCursorRevision,
          cursorAccountRefreshError: null,
        });
        if (CURSOR_OVERLAY_VIEW_SET.has(this.state.view)) {
          await this.refreshActiveView();
        }
      } catch (error) {
        this.patch({ cursorAccountRefreshError: humanStatus(error) });
        throw error;
      } finally {
        this.cursorRefreshInflight = null;
      }
    })();
    this.cursorRefreshInflight = run;
    return run;
  };

  refreshActiveView = async (
    overrideFilter?: Filter,
    overridePreset?: string,
  ): Promise<void> => {
    const view = this.state.view;
    const grain = this.state.grain;
    const activeScope = this.viewScopes[view];
    const nextFilter = overrideFilter ?? activeScope.filter;
    const nextPreset = overridePreset ?? activeScope.preset;
    const usedScope: ViewScope = { filter: nextFilter, preset: nextPreset };

    const generation = ++this.requestGeneration;
    this.wideRefreshGeneration = generation;
    const capturedRevisions: FreshnessRevisions = { ...this.revisions };

    const localOnly = LOCAL_ONLY_VIEWS.has(view);
    if (!localOnly && !this.hydratedViews.has(view)) {
      this.patch({ loading: true });
    }

    const commit = (update: Partial<InternalState>) => {
      if (generation === this.requestGeneration) {
        this.patch(update);
      }
    };

    const needsOptions = this.optionsEpoch !== capturedRevisions.data;
    const paint: Array<Promise<void>> = [];
    const tasks: Array<Promise<void>> = [];

    if (view === "overview") {
      const prev = previousFilter(nextFilter, nextPreset);
      const heat = heatmapFilter(nextFilter);
      paint.push(
        this.backend
          .getOverviewBundle({
            filter: nextFilter,
            previous_filter: prev,
            heatmap_filter: heat.filter,
            grain,
            top_sessions_limit: OVERVIEW_TOP_SESSIONS_LIMIT,
            include_options: needsOptions,
          })
          .then((bundle) => {
            if (generation !== this.requestGeneration) {
              return;
            }
            if (bundle.options !== null) {
              this.optionsEpoch = capturedRevisions.data;
            }
            commit({
              ...(bundle.options !== null ? { options: bundle.options } : {}),
              overview: bundle.overview,
              previous: bundle.previous,
              trend: bundle.trend,
              heatmap: bundle.heatmap,
              heatmapRange: { from: heat.fromDate, to: heat.toDate },
              models: bundle.models,
              projects: bundle.projects,
              sessions: bundle.sessions,
              billingWindows: bundle.billing_windows,
              cursorAccountUsage: bundle.cursor_account_usage,
              budgetStatus: bundle.budget_status,
            });
          }),
      );
      tasks.push(
        this.backend.getOfficialQuota().then((officialQuota) => {
          commit({ officialQuota });
          void this.backend
            .refreshOfficialQuota()
            .then((refreshed) => {
              commit({ officialQuota: refreshed });
            })
            .catch(() => undefined);
        }),
      );
    } else {
      if (needsOptions) {
        paint.push(
          this.backend.getFilterOptions().then((options) => {
            if (generation === this.requestGeneration) {
              this.optionsEpoch = capturedRevisions.data;
              commit({ options });
            }
          }),
        );
      }
      if (view === "trend") {
        tasks.push(
          this.backend.getTrend(nextFilter, grain).then((trend) => {
            commit({ trend });
          }),
        );
      }
      if (view === "application") {
        tasks.push(
          this.backend.getApplicationAnalytics(nextFilter, grain).then((applicationAnalytics) => {
            commit({ applicationAnalytics });
          }),
        );
      }
      if (view === "model") {
        tasks.push(
          this.backend.getBreakdown(nextFilter, "model").then((models) => {
            commit({ models });
          }),
        );
      }
      if (view === "provider") {
        tasks.push(
          this.backend.getBreakdown(nextFilter, "provider").then((providerBreakdown) => {
            commit({ providerBreakdown });
          }),
        );
      }
      if (view === "project") {
        tasks.push(
          this.backend.getBreakdown(nextFilter, "project").then((projects) => {
            commit({ projects });
          }),
        );
      }
      if (view === "cursor") {
        this.patch({ codeVolumeLoading: true });
        tasks.push(
          this.backend
            .getCodeVolume()
            .then((codeVolume) => {
              commit({ codeVolume });
            })
            .finally(() => {
              if (generation === this.requestGeneration) {
                this.patch({ codeVolumeLoading: false });
              }
            }),
        );
      }
      if (view === "cursor-sessions") {
        this.patch({ cursorSessionLoading: true });
        tasks.push(
          this.backend
            .getCursorSessionSummary()
            .then((cursorSessionSummary) => {
              commit({ cursorSessionSummary });
            })
            .finally(() => {
              if (generation === this.requestGeneration) {
                this.patch({ cursorSessionLoading: false });
              }
            }),
        );
      }
      if (view === "settings") {
        tasks.push(
          this.backend.getPrices().then((prices) => {
            commit({ prices });
          }),
          this.backend.getSourceDiagnostics().then((diagnostics) => {
            commit({ diagnostics });
          }),
          this.backend.getBudgetStatus().then((budgetStatus) => {
            commit({ budgetStatus });
          }),
          this.backend.getOfficialQuota().then((officialQuota) => {
            commit({ officialQuota });
          }),
        );
      }
    }

    try {
      if (paint.length > 0) {
        await Promise.all(paint);
      }
      if (
        generation === this.requestGeneration &&
        (view === "overview" || view === "cursor" || view === "cursor-sessions")
      ) {
        this.patch({ loading: false });
      }
      if (tasks.length > 0) {
        await Promise.all(tasks);
      }
      if (generation === this.requestGeneration) {
        this.markHydrated(view, usedScope, grain, capturedRevisions);
      }
    } finally {
      if (this.wideRefreshGeneration === generation) {
        this.wideRefreshGeneration = null;
      }
      if (generation === this.requestGeneration) {
        this.patch({ loading: false });
      }
    }

    if (generation === this.requestGeneration) {
      this.patch({ updatedAt: new Date().toISOString() });
    }
  };
}
