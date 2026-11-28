import type {
  ApplicationAnalyticsDto,
  BillingWindowsDto,
  BudgetConfig,
  BudgetStatusDto,
  CodeVolumeSummary,
  CursorAccountUsageDto,
  CursorSessionSummaryDto,
  Filter,
  FilterOptions,
  Grain,
  IngestIssue,
  IngestReport,
  NamedAmount,
  OfficialQuotaDto,
  OverviewBundleDto,
  OverviewBundleRequest,
  OverviewDto,
  PriceEntry,
  PriceTable,
  SeriesPoint,
  SessionRow,
  SourceDiagnostic,
} from "../../types";
import type { BreakdownDimension, UsageBackend } from "./usageBackend";

export type FakeUsageRecord = {
  occurred_at: string;
  source: string;
  model: string;
  provider: string;
  project: string;
  session_id: string;
  tokens: number;
};

export type FakeCursorEvent = {
  occurred_at: string;
  model: string;
  tokens: number;
};

export type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
};

export function createDeferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

export type FakeBackendSnapshot = {
  records: FakeUsageRecord[];
  userPrices: PriceEntry[];
  snapshotPrices: PriceEntry[];
  budget: BudgetConfig;
  cursorEvents: FakeCursorEvent[];
  cursorAsOf: string | null;
  diagnostics: SourceDiagnostic[];
  officialQuota: OfficialQuotaDto;
  conversationIssues: IngestIssue[];
};

type MethodName = keyof UsageBackend;

function priceEntry(model: string, inputPerToken: number, origin: "user" | "snapshot"): PriceEntry {
  return {
    model,
    provider: null,
    input: inputPerToken,
    output: 0,
    cache_read: 0,
    cache_creation: 0,
    origin,
  };
}

export function makeDefaultFakeSnapshot(): FakeBackendSnapshot {
  return {
    records: [
      {
        occurred_at: "2026-08-15T10:00:00.000Z",
        source: "claude",
        model: "claude-opus-4",
        provider: "anthropic",
        project: "/app",
        session_id: "sess-1",
        tokens: 1_000_000,
      },
      {
        occurred_at: "2026-08-16T10:00:00.000Z",
        source: "codex",
        model: "gpt-5",
        provider: "openai",
        project: "/lib",
        session_id: "sess-2",
        tokens: 500_000,
      },
    ],
    userPrices: [priceEntry("claude-opus-4", 2 / 1_000_000, "user")],
    snapshotPrices: [priceEntry("gpt-5", 1 / 1_000_000, "snapshot")],
    budget: { monthly_usd: 100 },
    cursorEvents: [
      {
        occurred_at: "2026-08-15T12:00:00.000Z",
        model: "gpt-5",
        tokens: 200_000,
      },
    ],
    cursorAsOf: "2026-08-16T12:00:00.000Z",
    diagnostics: [
      {
        source: "claude",
        application: "Claude Code",
        detected: true,
        root_path: "/home/.claude/projects",
        cached_files: 1,
        record_count: 1,
        total_tokens: 1_000_000,
        coverage: "2026-08-15",
        archived_record_count: 0,
      },
    ],
    officialQuota: {
      rows: [
        {
          provider: "claude",
          application: "Claude Code",
          windows: [],
          freshness: "official",
          captured_at: "2026-08-16T12:00:00.000Z",
          error: null,
          todo: null,
          plan: "Pro",
        },
      ],
      alerts_enabled: true,
      stale_after_minutes: 30,
      undetected: [],
      hidden_providers: [],
    },
    conversationIssues: [],
  };
}

export class StatefulUsageBackendFake implements UsageBackend {
  state: FakeBackendSnapshot;
  refreshedOfficialQuota: OfficialQuotaDto | null = null;
  refreshedCursorEvents: FakeCursorEvent[] | null = null;
  refreshedCursorAsOf = "2026-08-17T00:00:00.000Z";
  failMethod: Partial<Record<MethodName, Error>> = {};
  callCounts: Record<string, number> = {};
  recordedRequests: Array<{ method: MethodName; args: unknown[] }> = [];

  private readonly gates = new Map<MethodName, Deferred<void>[]>();

  constructor(initial?: Partial<FakeBackendSnapshot>) {
    const base = makeDefaultFakeSnapshot();
    this.state = {
      ...base,
      ...initial,
      records: [...(initial?.records ?? base.records)],
      userPrices: [...(initial?.userPrices ?? base.userPrices)],
      snapshotPrices: [...(initial?.snapshotPrices ?? base.snapshotPrices)],
      cursorEvents: [...(initial?.cursorEvents ?? base.cursorEvents)],
      diagnostics: [...(initial?.diagnostics ?? base.diagnostics)],
      conversationIssues: [...(initial?.conversationIssues ?? base.conversationIssues)],
    };
  }

  /** Next invocation of `method` suspends until the returned deferred resolves (or rejects). */
  gateNext(method: MethodName): Deferred<void> {
    const d = createDeferred<void>();
    const list = this.gates.get(method) ?? [];
    list.push(d);
    this.gates.set(method, list);
    return d;
  }

  count(method: MethodName): number {
    return this.callCounts[method] ?? 0;
  }

  resetCounts(): void {
    this.callCounts = {};
    this.recordedRequests = [];
  }

  applySnapshotPrices(entries: PriceEntry[]): void {
    this.state.snapshotPrices = [...entries];
  }

  applyBackupSnapshot(next: FakeBackendSnapshot): void {
    this.state = {
      ...next,
      records: [...next.records],
      userPrices: [...next.userPrices],
      snapshotPrices: [...next.snapshotPrices],
      cursorEvents: [...next.cursorEvents],
      diagnostics: [...next.diagnostics],
      conversationIssues: [...next.conversationIssues],
    };
  }

  private async intercept(method: MethodName, args: unknown[]): Promise<void> {
    this.callCounts[method] = (this.callCounts[method] ?? 0) + 1;
    this.recordedRequests.push({ method, args });
    const gate = this.gates.get(method)?.shift();
    if (gate) {
      await gate.promise;
    }
    const failure = this.failMethod[method];
    if (failure) {
      throw failure;
    }
  }

  private effectivePrice(model: string): { rate: number; origin: "user" | "snapshot" } | null {
    const lower = model.toLowerCase();
    const user = this.state.userPrices.find((p) => p.model.toLowerCase() === lower);
    if (user) {
      return { rate: user.input, origin: "user" };
    }
    const snap = this.state.snapshotPrices.find((p) => p.model.toLowerCase() === lower);
    if (snap) {
      return { rate: snap.input, origin: "snapshot" };
    }
    return null;
  }

  private filterRecords(filter?: Filter): FakeUsageRecord[] {
    if (!filter) {
      return this.state.records;
    }
    return this.state.records.filter((r) => {
      if (filter.from && r.occurred_at < filter.from) return false;
      if (filter.to && r.occurred_at > filter.to) return false;
      if (filter.sources.length > 0 && !filter.sources.includes(r.source)) return false;
      if (filter.models.length > 0 && !filter.models.includes(r.model)) return false;
      if (filter.projects.length > 0 && !filter.projects.includes(r.project)) return false;
      if (filter.providers.length > 0 && !filter.providers.includes(r.provider)) return false;
      return true;
    });
  }

  private filterCursorEvents(filter?: Filter): FakeCursorEvent[] {
    if (!filter) {
      return this.state.cursorEvents;
    }
    return this.state.cursorEvents.filter((e) => {
      if (filter.from && e.occurred_at < filter.from) return false;
      if (filter.to && e.occurred_at > filter.to) return false;
      if (filter.models.length > 0 && !filter.models.includes(e.model)) return false;
      return true;
    });
  }

  private recordCost(record: FakeUsageRecord): { cost: number | null; origin: "user" | "snapshot" | "none" } {
    const found = this.effectivePrice(record.model);
    if (!found) {
      return { cost: null, origin: "none" };
    }
    return { cost: record.tokens * found.rate, origin: found.origin };
  }

  private computeOverview(filter: Filter): OverviewDto {
    const matched = this.filterRecords(filter);
    let totalTokens = 0;
    let totalCost = 0;
    let hasCost = false;
    let userCost = 0;
    let hasUser = false;
    let snapCost = 0;
    let hasSnap = false;
    let unpricedRecords = 0;
    const sessions = new Set<string>();
    for (const r of matched) {
      totalTokens += r.tokens;
      sessions.add(`${r.source}:${r.session_id}`);
      const c = this.recordCost(r);
      if (c.cost != null) {
        totalCost += c.cost;
        hasCost = true;
        if (c.origin === "user") {
          userCost += c.cost;
          hasUser = true;
        } else if (c.origin === "snapshot") {
          snapCost += c.cost;
          hasSnap = true;
        }
      } else {
        unpricedRecords += 1;
      }
    }
    return {
      total_tokens: totalTokens,
      input_tokens: totalTokens,
      output_tokens: 0,
      cache_read_tokens: 0,
      cache_creation_tokens: 0,
      reasoning_tokens: 0,
      session_count: sessions.size,
      cost: hasCost ? totalCost : null,
      unpriced: unpricedRecords > 0,
      cost_breakdown: {
        input: hasCost ? totalCost : null,
        output: null,
        cache_read: null,
        cache_creation: null,
      },
      cost_sources: {
        native: null,
        user: hasUser ? userCost : null,
        snapshot: hasSnap ? snapCost : null,
        unpriced_records: unpricedRecords,
      },
    };
  }

  private computeTrend(filter: Filter, grain: Grain): SeriesPoint[] {
    const buckets = new Map<string, { tokens: number; cost: number; hasCost: boolean }>();
    const bucketOf = (iso: string) => {
      if (grain === "hour") return iso.slice(0, 13);
      if (grain === "month") return iso.slice(0, 7);
      if (grain === "week") return `${iso.slice(0, 4)}-W${grain}`;
      return iso.slice(0, 10);
    };
    for (const r of this.filterRecords(filter)) {
      const key = bucketOf(r.occurred_at);
      const entry = buckets.get(key) ?? { tokens: 0, cost: 0, hasCost: false };
      entry.tokens += r.tokens;
      const c = this.recordCost(r).cost;
      if (c != null) {
        entry.cost += c;
        entry.hasCost = true;
      }
      buckets.set(key, entry);
    }
    for (const e of this.filterCursorEvents(filter)) {
      const key = bucketOf(e.occurred_at);
      const entry = buckets.get(key) ?? { tokens: 0, cost: 0, hasCost: false };
      entry.tokens += e.tokens;
      const p = this.effectivePrice(e.model);
      if (p) {
        entry.cost += e.tokens * p.rate;
        entry.hasCost = true;
      }
      buckets.set(key, entry);
    }
    return [...buckets.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([bucket, v]) => ({
        bucket,
        total_tokens: v.tokens,
        input_tokens: v.tokens,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        cost: v.hasCost ? v.cost : null,
      }));
  }

  private computeBreakdown(filter: Filter, dimension: BreakdownDimension): NamedAmount[] {
    const groups = new Map<string, { tokens: number; cost: number; hasCost: boolean; unpriced: boolean }>();
    for (const r of this.filterRecords(filter)) {
      const key = dimension === "model" ? r.model : dimension === "provider" ? r.provider : r.project;
      const entry = groups.get(key) ?? { tokens: 0, cost: 0, hasCost: false, unpriced: false };
      entry.tokens += r.tokens;
      const c = this.recordCost(r).cost;
      if (c != null) {
        entry.cost += c;
        entry.hasCost = true;
      } else {
        entry.unpriced = true;
      }
      groups.set(key, entry);
    }
    if (dimension === "project") {
      const cursorEvents = this.filterCursorEvents(filter);
      if (cursorEvents.length > 0) {
        const entry = { tokens: 0, cost: 0, hasCost: false, unpriced: false };
        for (const e of cursorEvents) {
          entry.tokens += e.tokens;
          const p = this.effectivePrice(e.model);
          if (p) {
            entry.cost += e.tokens * p.rate;
            entry.hasCost = true;
          } else {
            entry.unpriced = true;
          }
        }
        groups.set("Cursor（账号用量）", entry);
      }
    }
    const grand = [...groups.values()].reduce((sum, item) => sum + item.tokens, 0);
    return [...groups.entries()]
      .map(([name, item]) => ({
        name,
        total_tokens: item.tokens,
        share: grand === 0 ? 0 : item.tokens / grand,
        cost: item.hasCost ? item.cost : null,
        unpriced: item.unpriced,
      }))
      .sort((a, b) => b.total_tokens - a.total_tokens || a.name.localeCompare(b.name));
  }

  private computeCursorSummary(filter?: Filter): CursorAccountUsageDto {
    const events = this.filterCursorEvents(filter);
    const totalTokens = events.reduce((sum, e) => sum + e.tokens, 0);
    return {
      as_of: this.state.cursorAsOf,
      event_count: events.length,
      input_tokens: totalTokens,
      output_tokens: 0,
      cache_read_tokens: 0,
      cache_creation_tokens: 0,
      total_tokens: totalTokens,
      daily: [],
      by_model: [],
      headless_tokens: 0,
      interactive_tokens: totalTokens,
      headless_share: events.length > 0 ? 0 : null,
    };
  }

  private computeBillingWindows(filter: Filter): BillingWindowsDto {
    const cursorEvents = this.filterCursorEvents({
      ...filter,
      from: null,
      to: null,
    });
    const weekly: BillingWindowsDto["weekly"] = [];
    if (cursorEvents.length > 0) {
      const tokens = cursorEvents.reduce((s, e) => s + e.tokens, 0);
      weekly.push({
        source: "cursor",
        application: "Cursor",
        window_days: 7,
        start: "2026-08-10T00:00:00.000Z",
        end: "2026-08-17T00:00:00.000Z",
        total_tokens: tokens,
        input_tokens: tokens,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        session_count: cursorEvents.length,
        cost: null,
        unpriced: false,
        daily_average_tokens: Math.round(tokens / 7),
        daily_average_cost: null,
      });
    }
    return {
      now: "2026-08-17T00:00:00.000Z",
      window_hours: 5,
      current: [],
      recent: [],
      weekly_window_days: 7,
      weekly,
    };
  }

  private computeBudgetStatus(): BudgetStatusDto {
    const all = this.computeOverview({
      from: null,
      to: null,
      sources: [],
      models: [],
      projects: [],
      providers: [],
    });
    const mtd = all.cost ?? 0;
    const budget = this.state.budget.monthly_usd;
    return {
      monthly_budget: budget,
      month: "2026-08",
      days_elapsed: 16,
      days_in_month: 31,
      month_to_date_cost: mtd,
      unpriced: all.unpriced,
      projected_month_cost: (mtd / 16) * 31,
      percent_used: budget && budget > 0 ? (mtd / budget) * 100 : null,
      percent_projected: budget && budget > 0 ? (((mtd / 16) * 31) / budget) * 100 : null,
      thresholds: [50, 80, 100],
    };
  }

  private computeOptions(): FilterOptions {
    return {
      sources: [...new Set(this.state.records.map((r) => r.source))].sort(),
      models: [...new Set(this.state.records.map((r) => r.model))].sort(),
      projects: [...new Set(this.state.records.map((r) => r.project))].sort(),
      providers: [...new Set(this.state.records.map((r) => r.provider))].sort(),
    };
  }

  private computeTopSessions(filter: Filter, limit: number): SessionRow[] {
    return this.filterRecords(filter)
      .slice(0, limit)
      .map((r) => {
        const c = this.recordCost(r);
        return {
          session_id: r.session_id,
          source: r.source,
          project: r.project,
          model: r.model,
          total_tokens: r.tokens,
          started_at: r.occurred_at,
          ended_at: r.occurred_at,
          source_file: `/${r.source}/${r.session_id}.jsonl`,
          cost: c.cost,
          unpriced: c.cost == null,
        };
      });
  }

  async ping(): Promise<string> {
    await this.intercept("ping", []);
    return "pong";
  }

  async getFilterOptions(): Promise<FilterOptions> {
    await this.intercept("getFilterOptions", []);
    return this.computeOptions();
  }

  async getOverviewBundle(request: OverviewBundleRequest): Promise<OverviewBundleDto> {
    await this.intercept("getOverviewBundle", [request]);
    return {
      options: request.include_options ? this.computeOptions() : null,
      overview: this.computeOverview(request.filter),
      previous: request.previous_filter ? this.computeOverview(request.previous_filter) : null,
      trend: this.computeTrend(request.filter, request.grain),
      heatmap: this.computeTrend(request.heatmap_filter, "day"),
      models: this.computeBreakdown(request.filter, "model"),
      projects: this.computeBreakdown(request.filter, "project"),
      sessions: this.computeTopSessions(request.filter, request.top_sessions_limit ?? 8),
      billing_windows: this.computeBillingWindows(request.filter),
      cursor_account_usage: this.computeCursorSummary(request.filter),
      budget_status: this.computeBudgetStatus(),
    };
  }

  async getTrend(filter: Filter, grain: Grain): Promise<SeriesPoint[]> {
    await this.intercept("getTrend", [filter, grain]);
    return this.computeTrend(filter, grain);
  }

  async getApplicationAnalytics(filter: Filter, grain: Grain): Promise<ApplicationAnalyticsDto> {
    await this.intercept("getApplicationAnalytics", [filter, grain]);
    const matched = this.filterRecords(filter);
    const bySource = new Map<string, number>();
    for (const r of matched) {
      bySource.set(r.source, (bySource.get(r.source) ?? 0) + r.tokens);
    }
    const cursorEvents = this.filterCursorEvents(filter);
    const cursorTokens = cursorEvents.reduce((s, e) => s + e.tokens, 0);
    const localTotal = matched.reduce((s, r) => s + r.tokens, 0);
    const byApplication: ApplicationAnalyticsDto["by_application"] = [...bySource.entries()].map(
      ([source, tokens]) => ({
        source,
        application: source,
        metrics: {
          total_tokens: tokens,
          session_count: 1,
          cache_hit_rate: null,
          average_session_tokens: tokens,
          reasoning_share: 0,
        },
      }),
    );
    if (cursorTokens > 0) {
      byApplication.push({
        source: "cursor",
        application: "Cursor",
        metrics: {
          total_tokens: cursorTokens,
          session_count: cursorEvents.length,
          cache_hit_rate: null,
          average_session_tokens: null,
          reasoning_share: null,
        },
      });
    }
    return {
      summary: {
        total_tokens: localTotal,
        session_count: matched.length,
        cache_hit_rate: null,
        average_session_tokens: matched.length > 0 ? localTotal / matched.length : null,
        reasoning_share: 0,
      },
      by_application: byApplication,
      trend: [],
      projects: [],
    };
  }

  async getBreakdown(filter: Filter, dimension: BreakdownDimension): Promise<NamedAmount[]> {
    await this.intercept("getBreakdown", [filter, dimension]);
    return this.computeBreakdown(filter, dimension);
  }

  async getCodeVolume(): Promise<CodeVolumeSummary> {
    await this.intercept("getCodeVolume", []);
    const all = this.computeOverview({
      from: null,
      to: null,
      sources: [],
      models: [],
      projects: [],
      providers: [],
    });
    return {
      commit_count: 1,
      lines_added: 100,
      lines_deleted: 10,
      net_lines: 90,
      composer_lines_added: 50,
      composer_lines_deleted: 5,
      human_lines_added: 30,
      human_lines_deleted: 5,
      tab_lines_added: 20,
      tab_lines_deleted: 0,
      ai_percentage: 50,
      total_cost: all.cost,
      cost_unpriced: all.unpriced,
      cost_per_thousand_ai_lines: all.cost != null ? (all.cost / 50) * 1000 : null,
      daily: [],
      by_branch: [],
      commits: [],
    };
  }

  async getCursorSessionSummary(): Promise<CursorSessionSummaryDto> {
    await this.intercept("getCursorSessionSummary", []);
    return {
      as_of: "2026-08-16T12:00:00.000Z",
      session_count: 2,
      turn_count: 10,
      aborted_count: 0,
      user_prompt_count: 4,
      subagent_count: 0,
      error_rate: 0,
      average_turns: 5,
      single_prompt_ratio: 0.5,
      average_tools_per_turn: 1,
      write_read_ratio: 0.5,
      active_project_count: 1,
      by_project: [],
      by_model: [],
      by_source: [],
      by_extension: [],
      top_tools: [],
      tool_groups: [],
      daily: [],
    };
  }

  async getPrices(): Promise<PriceTable> {
    await this.intercept("getPrices", []);
    return { prices: [...this.state.userPrices] };
  }

  async savePriceTable(prices: PriceTable): Promise<void> {
    await this.intercept("savePriceTable", [prices]);
    this.state.userPrices = [...prices.prices];
  }

  async getSourceDiagnostics(): Promise<SourceDiagnostic[]> {
    await this.intercept("getSourceDiagnostics", []);
    return [...this.state.diagnostics];
  }

  async getBudgetStatus(): Promise<BudgetStatusDto> {
    await this.intercept("getBudgetStatus", []);
    return this.computeBudgetStatus();
  }

  async saveBudget(config: BudgetConfig): Promise<void> {
    await this.intercept("saveBudget", [config]);
    this.state.budget = { ...config };
  }

  async getOfficialQuota(): Promise<OfficialQuotaDto> {
    await this.intercept("getOfficialQuota", []);
    return this.state.officialQuota;
  }

  async refreshOfficialQuota(): Promise<OfficialQuotaDto> {
    await this.intercept("refreshOfficialQuota", []);
    if (this.refreshedOfficialQuota) {
      this.state.officialQuota = this.refreshedOfficialQuota;
    }
    return this.state.officialQuota;
  }

  async getCursorAccountUsage(filter?: Filter): Promise<CursorAccountUsageDto> {
    await this.intercept("getCursorAccountUsage", [filter]);
    return this.computeCursorSummary(filter);
  }

  async refreshCursorAccountUsage(): Promise<CursorAccountUsageDto> {
    await this.intercept("refreshCursorAccountUsage", []);
    if (this.refreshedCursorEvents) {
      this.state.cursorEvents = [...this.refreshedCursorEvents];
      this.state.cursorAsOf = this.refreshedCursorAsOf;
    }
    return this.computeCursorSummary();
  }

  async clearCursorAccountUsage(): Promise<CursorAccountUsageDto> {
    await this.intercept("clearCursorAccountUsage", []);
    this.state.cursorEvents = [];
    this.state.cursorAsOf = null;
    return this.computeCursorSummary();
  }

  async getConversationCatalogIssues(): Promise<IngestIssue[]> {
    await this.intercept("getConversationCatalogIssues", []);
    return [...this.state.conversationIssues];
  }

  async ingest(): Promise<IngestReport> {
    await this.intercept("ingest", []);
    return {
      files_seen: 1,
      files_parsed: 1,
      files_skipped: 0,
      files_failed: 0,
      records_written: this.state.records.length,
      records_removed: 0,
      records_archived: 0,
      partial_success: false,
      issues: [],
      sources: [],
      conversation_issues: [...this.state.conversationIssues],
    };
  }

  async rebuildCache(source: string | null): Promise<IngestReport> {
    await this.intercept("rebuildCache", [source]);
    return {
      files_seen: 1,
      files_parsed: 1,
      files_skipped: 0,
      files_failed: 0,
      records_written: this.state.records.length,
      records_removed: 0,
      records_archived: 0,
      partial_success: false,
      issues: [],
      sources: [],
      conversation_issues: [...this.state.conversationIssues],
    };
  }

  async purgeArchivedRecords(source: string | null): Promise<number> {
    await this.intercept("purgeArchivedRecords", [source]);
    return 0;
  }

  async refreshTray(): Promise<void> {
    await this.intercept("refreshTray", []);
  }
}
