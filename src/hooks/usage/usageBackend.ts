import { invoke } from "@tauri-apps/api/core";
import type {
  ApplicationAnalyticsDto,
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
  OfficialQuotaConfig,
  OfficialQuotaConfigPatch,
  OfficialQuotaDto,
  OverviewBundleDto,
  OverviewBundleRequest,
  PriceTable,
  SeriesPoint,
  SourceDiagnostic,
} from "../../types";

export type BreakdownDimension = "model" | "provider" | "project";

export interface UsageBackend {
  ping(): Promise<string>;
  getFilterOptions(): Promise<FilterOptions>;
  getOverviewBundle(request: OverviewBundleRequest): Promise<OverviewBundleDto>;
  getTrend(filter: Filter, grain: Grain): Promise<SeriesPoint[]>;
  getApplicationAnalytics(filter: Filter, grain: Grain): Promise<ApplicationAnalyticsDto>;
  getBreakdown(filter: Filter, dimension: BreakdownDimension): Promise<NamedAmount[]>;
  getCodeVolume(): Promise<CodeVolumeSummary>;
  getCursorSessionSummary(): Promise<CursorSessionSummaryDto>;
  getPrices(): Promise<PriceTable>;
  savePriceTable(prices: PriceTable): Promise<void>;
  getSourceDiagnostics(): Promise<SourceDiagnostic[]>;
  getBudgetStatus(): Promise<BudgetStatusDto>;
  saveBudget(config: BudgetConfig): Promise<void>;
  getOfficialQuota(): Promise<OfficialQuotaDto>;
  refreshOfficialQuota(): Promise<OfficialQuotaDto>;
  patchOfficialQuotaConfig(patch: OfficialQuotaConfigPatch): Promise<OfficialQuotaConfig>;
  getCursorAccountUsage(filter?: Filter): Promise<CursorAccountUsageDto>;
  refreshCursorAccountUsage(): Promise<CursorAccountUsageDto>;
  clearCursorAccountUsage(): Promise<CursorAccountUsageDto>;
  getConversationCatalogIssues(): Promise<IngestIssue[]>;
  ingest(): Promise<IngestReport>;
  rebuildCache(source: string | null): Promise<IngestReport>;
  purgeArchivedRecords(source: string | null): Promise<number>;
  refreshTray(): Promise<void>;
}

export const tauriUsageBackend: UsageBackend = {
  ping: () => invoke<string>("ping"),
  getFilterOptions: () => invoke<FilterOptions>("get_filter_options"),
  getOverviewBundle: (request) => invoke<OverviewBundleDto>("get_overview_bundle", { request }),
  getTrend: (filter, grain) => invoke<SeriesPoint[]>("get_trend", { filter, grain }),
  getApplicationAnalytics: (filter, grain) =>
    invoke<ApplicationAnalyticsDto>("get_application_analytics", { filter, grain }),
  getBreakdown: (filter, dimension) =>
    invoke<NamedAmount[]>("get_breakdown", { query: { filter, dimension } }),
  getCodeVolume: () => invoke<CodeVolumeSummary>("get_code_volume"),
  getCursorSessionSummary: () => invoke<CursorSessionSummaryDto>("get_cursor_session_summary"),
  getPrices: () => invoke<PriceTable>("get_prices"),
  savePriceTable: (prices) => invoke<void>("save_price_table", { prices }),
  getSourceDiagnostics: () => invoke<SourceDiagnostic[]>("get_source_diagnostics"),
  getBudgetStatus: () => invoke<BudgetStatusDto>("get_budget_status"),
  saveBudget: (config) => invoke<void>("save_budget", { config }),
  getOfficialQuota: () => invoke<OfficialQuotaDto>("get_official_quota"),
  refreshOfficialQuota: () => invoke<OfficialQuotaDto>("refresh_official_quota"),
  patchOfficialQuotaConfig: (patch) =>
    invoke<OfficialQuotaConfig>("patch_official_quota_config", { patch }),
  getCursorAccountUsage: (filter) =>
    filter === undefined
      ? invoke<CursorAccountUsageDto>("get_cursor_account_usage")
      : invoke<CursorAccountUsageDto>("get_cursor_account_usage", { filter }),
  refreshCursorAccountUsage: () => invoke<CursorAccountUsageDto>("refresh_cursor_account_usage"),
  clearCursorAccountUsage: () => invoke<CursorAccountUsageDto>("clear_cursor_account_usage"),
  getConversationCatalogIssues: () => invoke<IngestIssue[]>("get_conversation_catalog_issues"),
  ingest: () => invoke<IngestReport>("ingest"),
  rebuildCache: (source) => invoke<IngestReport>("rebuild_cache", { source }),
  purgeArchivedRecords: (source) => invoke<number>("purge_archived_records", { source }),
  refreshTray: () => invoke<void>("refresh_tray"),
};
