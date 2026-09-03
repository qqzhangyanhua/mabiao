import { memo, useMemo, useState } from "react";
import { heatmapGrid } from "../lib/calendar";
import { chartPalette } from "../lib/chartTheme";
import type { ResolvedTheme } from "../hooks/useTheme";
import {
  collectPresentSources,
  filterOfficialQuotaRows,
  filterQuotaItems,
  isModuleVisible,
  isOfficialProviderVisible,
  visibleModuleCount,
  type OverviewLayout,
} from "../lib/overviewLayout";
import { Button } from "./ui/Button";
import { ReportDialog } from "./ReportDialog";
import { EmptyState } from "./EmptyState";
import { OverviewKpiSection, OverviewStatusBar } from "./OverviewKpiSection";
import { OverviewLayoutBar } from "./OverviewLayoutBar";
import { OverviewPanels } from "./OverviewPanels";
import {
  cacheHitRate,
  deltaPct,
  formatClock,
  formatCompact,
  formatDelta,
  formatPercent,
  formatUsd,
} from "../lib/format";
import { formatCostBucketLine, formatCostSourceLine } from "../lib/costBreakdown";
import { costEstimateKpiLink } from "../lib/unpricedKpi";
import type {
  BillingWindowsDto,
  Grain,
  OfficialQuotaDto,
  NamedAmount,
  CursorAccountUsageDto,
  OverviewDto,
  SeriesPoint,
  SessionRow,
} from "../types";

/** 各粒度下一个 trend bucket 覆盖的分钟数，用于把「最后一个 bucket 的 token 量」换算成「每分钟速率」。 */
const BUCKET_MINUTES: Record<Grain, number> = {
  hour: 60,
  day: 24 * 60,
  week: 7 * 24 * 60,
  month: 30 * 24 * 60,
};

const emptyOverview: OverviewDto = {
  total_tokens: 0,
  input_tokens: 0,
  output_tokens: 0,
  cache_read_tokens: 0,
  cache_creation_tokens: 0,
  reasoning_tokens: 0,
  session_count: 0,
  cost: null,
  unpriced: false,
  cost_breakdown: {
    input: null,
    output: null,
    cache_read: null,
    cache_creation: null,
  },
  cost_sources: {
    native: null,
    user: null,
    snapshot: null,
    unpriced_records: 0,
  },
};

export const Overview = memo(function Overview({
  overview,
  billingWindows,
  officialQuota,
  cursorAccountUsage,
  previous,
  trend,
  heatmap,
  heatmapRange,
  models,
  projects,
  sessions,
  grain,
  preset,
  updatedAt,
  live,
  theme,
  onGrain,
  onOpenConversations,
  onOpenCursor,
  onProjectClick,
  onSessionClick,
  onOpenWorktime,
  onRangeSelect,
  onRangeBack,
  onModelClick,
  layout,
  onLayoutChange,
  detectedSources,
  onOfficialQuota,
  onQuotaError,
  onOpenUnpricedDiagnosis,
}: {
  overview: OverviewDto | null;
  billingWindows: BillingWindowsDto | null;
  officialQuota: OfficialQuotaDto | null;
  cursorAccountUsage: CursorAccountUsageDto | null;
  previous: OverviewDto | null;
  trend: SeriesPoint[];
  heatmap: SeriesPoint[];
  heatmapRange: { from: string; to: string };
  models: NamedAmount[];
  projects: NamedAmount[];
  sessions: SessionRow[];
  grain: Grain;
  preset: string;
  updatedAt: string | null;
  live: boolean;
  theme: ResolvedTheme;
  onGrain: (grain: Grain) => void;
  onOpenConversations: () => void;
  onOpenCursor: () => void;
  onProjectClick?: (project: string) => void;
  onSessionClick?: (session: { id: string; source: string }) => void;
  onOpenWorktime?: (day: string) => void;
  onRangeSelect?: (from: string, to: string) => void;
  onRangeBack?: () => void;
  onModelClick?: (model: string) => void;
  layout: OverviewLayout;
  onLayoutChange: (layout: OverviewLayout) => void;
  detectedSources: string[];
  onOfficialQuota: (value: OfficialQuotaDto) => void;
  onQuotaError: (error: unknown) => void;
  onOpenUnpricedDiagnosis?: () => void;
}) {
  const data = overview ?? emptyOverview;
  const palette = chartPalette(theme);
  const days = periodDays(preset, grain, trend.length);
  const dailyAvg = data.total_tokens / days;
  const costLink = costEstimateKpiLink(data.unpriced);
  const costDiagnosis =
    costLink.actionLabel != null && onOpenUnpricedDiagnosis != null
      ? onOpenUnpricedDiagnosis
      : undefined;
  const costBucketLine = formatCostBucketLine(data.cost_breakdown, data.cost_sources.native);
  const costSourceLine = formatCostSourceLine(data.cost_sources);
  const costExtraLine = [costLink.hint, costBucketLine, costSourceLine]
    .filter((part): part is string => Boolean(part))
    .join(" · ");
  const last = trend[trend.length - 1];
  const rate = last ? Math.round(last.total_tokens / BUCKET_MINUTES[grain]) : 0;
  const spark = trend.map((point) => point.total_tokens);
  const presentSources = useMemo(
    () =>
      collectPresentSources(detectedSources, [
        ...(billingWindows?.current ?? []),
        ...(billingWindows?.weekly ?? []),
      ]),
    [billingWindows, detectedSources],
  );
  const visibleBilling = useMemo(() => {
    if (!billingWindows) {
      return null;
    }
    return {
      ...billingWindows,
      current: filterQuotaItems(billingWindows.current, layout),
      recent: filterQuotaItems(billingWindows.recent, layout),
      weekly: filterQuotaItems(billingWindows.weekly, layout),
    };
  }, [billingWindows, layout]);
  const visibleOfficialQuota = useMemo(() => {
    if (!officialQuota) {
      return null;
    }
    return {
      ...officialQuota,
      rows: filterOfficialQuotaRows(officialQuota.rows, layout),
      undetected: officialQuota.undetected.filter((id) => isOfficialProviderVisible(layout, id)),
    };
  }, [officialQuota, layout]);
  const activeWindows = (visibleBilling?.current ?? []).filter((window) => window.is_active).length;
  const weeklyDays = visibleBilling?.weekly_window_days ?? 7;
  const weeklyCount = visibleBilling?.weekly.length ?? 0;
  const heatmapWeeks = heatmapGrid(heatmapRange.from, heatmapRange.to).length;
  const tokenDelta = formatDelta(deltaPct(data.total_tokens, previous?.total_tokens ?? null));
  const costDelta =
    data.cost == null ? null : formatDelta(deltaPct(data.cost, previous?.cost ?? null));
  const cacheHitRateLabel = formatPercent(cacheHitRate(data.cache_read_tokens, data.input_tokens));
  const [reportOpen, setReportOpen] = useState(false);

  if (!overview) {
    return (
      <div className="dash">
        <EmptyState icon="tokens" title="正在加载总览数据…" />
      </div>
    );
  }

  const showKpi = isModuleVisible(layout, "kpi");
  const showOfficial = isModuleVisible(layout, "official");
  const showCursorAccount = isModuleVisible(layout, "cursorAccount");
  const showBilling = isModuleVisible(layout, "billing");
  const showWeekly = isModuleVisible(layout, "weekly");
  const showTrend = isModuleVisible(layout, "trend");
  const showHeatmap = isModuleVisible(layout, "heatmap");
  const showDetail = isModuleVisible(layout, "detail");
  const showStatus = isModuleVisible(layout, "status");
  const hasVisibleModule = visibleModuleCount(layout) > 0;

  return (
    <div className="dash">
      <div className="overview-head">
        <OverviewLayoutBar
          layout={layout}
          detectedSources={detectedSources}
          presentSources={presentSources}
          onChange={onLayoutChange}
        />
        <div className="overview-report-entry">
          <Button variant="accent" onClick={() => setReportOpen(true)}>
            生成周报
          </Button>
        </div>
      </div>
      {reportOpen ? <ReportDialog onClose={() => setReportOpen(false)} /> : null}
      {!hasVisibleModule ? (
        <EmptyState
          icon="overview"
          title="所有概览模块已隐藏"
          hint="点上方「配置显示」重新打开指标、额度或其它区块"
        />
      ) : null}
      {showKpi ? (
        <OverviewKpiSection
          tokenValue={formatCompact(data.total_tokens)}
          tokenDelta={tokenDelta}
          sessionValue={data.session_count.toLocaleString("zh-CN")}
          sessionDelta={formatDelta(deltaPct(data.session_count, previous?.session_count ?? null))}
          costValue={formatUsd(data.cost, data.unpriced)}
          costDelta={costDiagnosis ? null : costDelta}
          costTitle={costExtraLine || undefined}
          costDetail={costExtraLine ? <p>{costExtraLine}</p> : undefined}
          costActionLabel={costDiagnosis ? costLink.actionLabel : undefined}
          onCostClick={costDiagnosis}
          dailyValue={formatCompact(Math.round(dailyAvg))}
          dailyDelta={formatDelta(
            deltaPct(dailyAvg, previous ? previous.total_tokens / Math.max(days, 1) : null),
          )}
          spark={spark}
          costSpark={trend.map((point) => point.cost ?? 0)}
          live={live}
        />
      ) : null}

      <OverviewPanels
        showOfficial={showOfficial}
        showCursorAccount={showCursorAccount}
        showBilling={showBilling}
        showWeekly={showWeekly}
        showTrend={showTrend}
        showHeatmap={showHeatmap}
        showDetail={showDetail}
        officialQuota={visibleOfficialQuota}
        cursorAccountUsage={cursorAccountUsage}
        billing={visibleBilling}
        weeklyDays={weeklyDays}
        weeklyCount={weeklyCount}
        activeWindows={activeWindows}
        trend={trend}
        models={models}
        heatmap={heatmap}
        heatmapRange={heatmapRange}
        heatmapWeeks={heatmapWeeks}
        data={data}
        projects={projects}
        sessions={sessions}
        grain={grain}
        theme={theme}
        onOfficialQuota={onOfficialQuota}
        onQuotaError={onQuotaError}
        onOpenCursor={onOpenCursor}
        onModelClick={onModelClick}
        onGrain={onGrain}
        onRangeSelect={onRangeSelect}
        onRangeBack={onRangeBack}
        onOpenWorktime={onOpenWorktime}
        onOpenConversations={onOpenConversations}
        onProjectClick={onProjectClick}
        onSessionClick={onSessionClick}
      />

      {showStatus ? (
        <OverviewStatusBar
          costValue={formatUsd(data.cost, data.unpriced)}
          costSourceLine={costSourceLine}
          unpriced={data.unpriced}
          cacheRead={formatCompact(data.cache_read_tokens)}
          cacheCreation={formatCompact(data.cache_creation_tokens)}
          reasoning={formatCompact(data.reasoning_tokens)}
          cacheHitRateLabel={cacheHitRateLabel}
          rate={rate.toLocaleString("zh-CN")}
          spark={spark}
          sparkColor={palette.output}
          updatedAt={formatClock(updatedAt)}
        />
      ) : null}
    </div>
  );
});

function periodDays(preset: string, grain: Grain, bucketCount: number): number {
  if (preset === "today") {
    return 1;
  }
  if (preset === "month") {
    return Math.max(new Date().getDate(), 1);
  }
  if (preset === "7") {
    return 7;
  }
  if (preset === "30") {
    return 30;
  }
  if (grain === "week") {
    return Math.max(bucketCount * 7, 1);
  }
  if (grain === "month") {
    return Math.max(bucketCount * 30, 1);
  }
  if (grain === "hour") {
    return Math.max(bucketCount / 24, 1);
  }
  return Math.max(bucketCount, 1);
}
