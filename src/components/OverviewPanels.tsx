import type { ResolvedTheme } from "../hooks/useTheme";
import type {
  BillingWindowsDto,
  CursorAccountUsageDto,
  Grain,
  NamedAmount,
  OfficialQuotaDto,
  OverviewDto,
  SeriesPoint,
  SessionRow,
} from "../types";
import { ActivityHeatmap } from "./ActivityHeatmap";
import { BillingWindows } from "./BillingWindows";
import { CollapsibleSection } from "./CollapsibleSection";
import { CursorOverviewPanel } from "./CursorOverviewPanel";
import { OfficialQuotaPanel } from "./OfficialQuotaPanel";
import { OverviewDetail } from "./OverviewDetail";
import { OverviewTrend } from "./OverviewTrend";
import { WeeklyWindows } from "./WeeklyWindows";

export function OverviewPanels({
  showOfficial,
  showCursorAccount,
  showBilling,
  showWeekly,
  showTrend,
  showHeatmap,
  showDetail,
  officialQuota,
  cursorAccountUsage,
  billing,
  weeklyDays,
  weeklyCount,
  activeWindows,
  trend,
  models,
  heatmap,
  heatmapRange,
  heatmapWeeks,
  data,
  projects,
  sessions,
  grain,
  theme,
  onOfficialQuota,
  onQuotaError,
  onOpenCursor,
  onModelClick,
  onGrain,
  onRangeSelect,
  onRangeBack,
  onOpenWorktime,
  onOpenConversations,
  onProjectClick,
  onSessionClick,
}: {
  showOfficial: boolean;
  showCursorAccount: boolean;
  showBilling: boolean;
  showWeekly: boolean;
  showTrend: boolean;
  showHeatmap: boolean;
  showDetail: boolean;
  officialQuota: OfficialQuotaDto | null;
  cursorAccountUsage: CursorAccountUsageDto | null;
  billing: BillingWindowsDto | null;
  weeklyDays: number;
  weeklyCount: number;
  activeWindows: number;
  trend: SeriesPoint[];
  models: NamedAmount[];
  heatmap: SeriesPoint[];
  heatmapRange: { from: string; to: string };
  heatmapWeeks: number;
  data: OverviewDto;
  projects: NamedAmount[];
  sessions: SessionRow[];
  grain: Grain;
  theme: ResolvedTheme;
  onOfficialQuota: (value: OfficialQuotaDto) => void;
  onQuotaError: (error: unknown) => void;
  onOpenCursor: () => void;
  onModelClick?: (model: string) => void;
  onGrain: (grain: Grain) => void;
  onRangeSelect?: (from: string, to: string) => void;
  onRangeBack?: () => void;
  onOpenWorktime?: (day: string) => void;
  onOpenConversations: () => void;
  onProjectClick?: (project: string) => void;
  onSessionClick?: (session: { id: string; source: string }) => void;
}) {
  return (
    <>
      {showOfficial ? (
        <OfficialQuotaPanel data={officialQuota} onQuota={onOfficialQuota} onError={onQuotaError} />
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
          <BillingWindows data={billing} />
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
          <WeeklyWindows windows={billing?.weekly ?? []} windowDays={weeklyDays} />
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
    </>
  );
}