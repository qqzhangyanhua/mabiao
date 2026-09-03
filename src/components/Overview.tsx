import { memo, useMemo, useState } from "react";
import { Icon } from "../icons";
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
import { ActivityHeatmap } from "./ActivityHeatmap";
import { BillingWindows } from "./BillingWindows";
import { CollapsibleSection } from "./CollapsibleSection";
import { CursorOverviewPanel } from "./CursorOverviewPanel";
import { OfficialQuotaPanel } from "./OfficialQuotaPanel";
import { ReportDialog } from "./ReportDialog";
import { EmptyState } from "./EmptyState";
import { KpiCard, Spark } from "./Kpi";
import { OverviewDetail } from "./OverviewDetail";
import { OverviewLayoutBar } from "./OverviewLayoutBar";
import { OverviewTrend } from "./OverviewTrend";
import { WeeklyWindows } from "./WeeklyWindows";
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
        <section className="kpi-row">
          <KpiCard
            icon="tokens"
            tone="purple"
            label="总 Token 使用量"
            value={formatCompact(data.total_tokens)}
            delta={tokenDelta}
            spark={spark}
          />
          <KpiCard
            icon="chat"
            tone="cyan"
            label="总会话数"
            value={data.session_count.toLocaleString("zh-CN")}
            delta={formatDelta(deltaPct(data.session_count, previous?.session_count ?? null))}
            spark={spark}
          />
          <KpiCard
            icon="cost"
            tone="orange"
            label="总费用估算"
            value={formatUsd(data.cost, data.unpriced)}
            delta={costDiagnosis ? null : costDelta}
            spark={trend.map((point) => point.cost ?? 0)}
            title={costExtraLine || undefined}
            detail={costExtraLine ? <p>{costExtraLine}</p> : undefined}
            actionLabel={costDiagnosis ? costLink.actionLabel : undefined}
            onClick={costDiagnosis}
          />
          <KpiCard
            icon="daily"
            tone="blue"
            label="日均 Token 使用量"
            value={formatCompact(Math.round(dailyAvg))}
            delta={formatDelta(
              deltaPct(dailyAvg, previous ? previous.total_tokens / Math.max(days, 1) : null),
            )}
            spark={spark}
            live={live}
            radar
          />
        </section>
      ) : null}

      {showOfficial ? (
        <OfficialQuotaPanel
          data={visibleOfficialQuota}
          onQuota={onOfficialQuota}
          onError={onQuotaError}
        />
      ) : null}

      {showCursorAccount ? (
        <CursorOverviewPanel
          data={cursorAccountUsage}
          onOpenCursor={onOpenCursor}
          onModelClick={onModelClick}
        />
      ) : null}

      {showBilling ? (
        <CollapsibleSection
          sectionId="billing"
          title="5 小时计费窗"
          className="panel billing-panel"
          extra={
            <span className="muted">
              由本地时间戳估计，非官方配额 · 始终展示最近窗口，不受时间范围筛选影响
            </span>
          }
          collapsedSummary={
            activeWindows > 0 ? `${activeWindows} 个进行中窗口` : "当前没有进行中的窗口"
          }
        >
          <BillingWindows data={visibleBilling} />
        </CollapsibleSection>
      ) : null}

      {showWeekly ? (
        <CollapsibleSection
          sectionId="weekly"
          title={`${weeklyDays} 天滚动用量`}
          className="panel weekly-panel"
          extra={
            <span className="muted">
              按来源统计最近 {weeklyDays} 天的累计消耗；Cursor 来自账号用量，费用按价目 / LiteLLM
              估算，非官方配额
            </span>
          }
          collapsedSummary={`${weeklyCount} 个 ${weeklyDays} 天窗口`}
        >
          <WeeklyWindows windows={visibleBilling?.weekly ?? []} windowDays={weeklyDays} />
        </CollapsibleSection>
      ) : null}

      {showTrend ? (
        <CollapsibleSection
          sectionId="trend"
          title="趋势与模型"
          className="collapsible-trend"
          collapsedSummary={`趋势 ${trend.length} 点 · 模型 ${models.length} 个`}
        >
          <OverviewTrend
            trend={trend}
            models={models}
            totalTokens={data.total_tokens}
            grain={grain}
            theme={theme}
            onGrain={onGrain}
            onRangeSelect={onRangeSelect}
            onRangeBack={onRangeBack}
            onModelClick={onModelClick}
          />
        </CollapsibleSection>
      ) : null}

      {showHeatmap ? (
        <CollapsibleSection
          sectionId="heatmap"
          title="活跃热力图"
          className="panel heatmap-panel"
          extra={
            <span className="muted">
              {heatmap.some((point) => point.total_tokens > 0)
                ? "近 53 周 · 按日 Token · 点击打开当天时间线"
                : "近 53 周暂无 Token"}
            </span>
          }
          collapsedSummary={`${heatmapWeeks} 周热力图`}
        >
          <ActivityHeatmap points={heatmap} range={heatmapRange} onDayClick={onOpenWorktime} />
        </CollapsibleSection>
      ) : null}

      {showDetail ? (
        <CollapsibleSection
          sectionId="detail"
          title="明细"
          className="collapsible-detail"
          collapsedSummary="Token 统计 · Top 项目 · 最近会话"
        >
          <OverviewDetail
            data={data}
            projects={projects}
            sessions={sessions}
            theme={theme}
            onOpenConversations={onOpenConversations}
            onProjectClick={onProjectClick}
            onSessionClick={onSessionClick}
          />
        </CollapsibleSection>
      ) : null}

      {showStatus ? (
        <footer className="status-bar">
          <div className="stat-block">
            <span className="muted">费用（估算）</span>
            <strong>{formatUsd(data.cost, data.unpriced)}</strong>
            <em>{costSourceLine ?? (data.unpriced ? "部分模型单价未配置" : "已按单价核算")}</em>
          </div>
          <div className="stat-block">
            <span className="muted">缓存 / 推理</span>
            <strong>
              {formatCompact(data.cache_read_tokens)} / {formatCompact(data.cache_creation_tokens)}{" "}
              / {formatCompact(data.reasoning_tokens)}
            </strong>
            <em>读 / 写 / 推理 · 命中率 {cacheHitRateLabel}（近似口径）</em>
          </div>
          <div className="stat-block">
            <span className="muted">Token 速率（估算）</span>
            <strong>
              {rate.toLocaleString("zh-CN")} <small>/min</small>
            </strong>
            <Spark values={spark} color={palette.output} />
          </div>
          <div className="stat-block last">
            <span className="muted">
              <Icon name="clock" size={13} /> 数据更新时间
            </span>
            <strong className="clock">{formatClock(updatedAt)}</strong>
          </div>
        </footer>
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
