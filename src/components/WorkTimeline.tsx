import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../icons";
import { hashForWorktime, parseWorktimeDay, replaceLocationHash } from "../hooks/viewCache";
import { parseDateValue, toDateValue } from "../lib/calendar";
import {
  sourceLabel,
  formatClock,
  formatCompact,
  formatDuration,
  formatHoursMinutes,
  formatRatio,
  formatTokens,
  humanStatus,
  projectLabel,
} from "../lib/format";
import { dayStartIso, laneCount, layoutSegments, type LaneSegment } from "../lib/workTimeline";
import type { WorkSegment, WorkTimelineDto } from "../types";
import { DatePicker } from "./ui/DatePicker";
import { EmptyState } from "./EmptyState";
import { KpiCard } from "./Kpi";
import { SourceLabel } from "./SourceIcon";
import { LoadingOverlay } from "./LoadingOverlay";
import { SESSION_ENTRY_COPY } from "../lib/sessionEntryCopy";

const AXIS_HOURS = [0, 3, 6, 9, 12, 15, 18, 21, 24];
const LANE_HEIGHT = 34;
const TOOLTIP_WIDTH = 260;
const TOOLTIP_HEIGHT = 200;
const TOOLTIP_MARGIN = 14;

function shiftDay(day: string, delta: number): string {
  const date = parseDateValue(day) ?? new Date();
  date.setDate(date.getDate() + delta);
  return toDateValue(date);
}

function segmentLabel(segment: WorkSegment): string {
  return `${projectLabel(segment.project)} · ${sourceLabel(segment.source)}/${segment.model}`;
}

export function WorkTimeline({
  initialDay,
  onSessionClick,
}: {
  initialDay?: string | null;
  onSessionClick?: (session: { id: string; source: string }) => void;
}) {
  const [day, setDay] = useState(
    () => initialDay ?? parseWorktimeDay(window.location.hash) ?? toDateValue(new Date()),
  );
  const [data, setData] = useState<WorkTimelineDto | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [hovered, setHovered] = useState<WorkSegment | null>(null);
  const [tooltipPos, setTooltipPos] = useState<{ x: number; y: number } | null>(null);
  const generationRef = useRef(0);

  useEffect(() => {
    const generation = ++generationRef.current;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 发起请求前先置 loading
    setLoading(true);
    setError(null);
    invoke<WorkTimelineDto>("get_work_timeline", { day })
      .then((next) => {
        if (generation === generationRef.current) {
          setData(next);
        }
      })
      .catch((err: unknown) => {
        if (generation === generationRef.current) {
          setError(humanStatus(err));
        }
      })
      .finally(() => {
        if (generation === generationRef.current) {
          setLoading(false);
        }
      });
  }, [day]);

  useEffect(() => {
    replaceLocationHash(hashForWorktime(day));
  }, [day]);

  const segments = data?.segments ?? [];
  const layout = layoutSegments(segments, dayStartIso(day));
  const lanes = Math.max(1, laneCount(layout));

  function handleSegmentEnter(event: React.MouseEvent, item: LaneSegment) {
    setHovered(item.segment);
    setTooltipPos({ x: event.clientX, y: event.clientY });
  }

  function handleSegmentMove(event: React.MouseEvent) {
    if (hovered) {
      setTooltipPos({ x: event.clientX, y: event.clientY });
    }
  }

  function handleSegmentLeave() {
    setHovered(null);
    setTooltipPos(null);
  }

  function handleSegmentClick(item: LaneSegment) {
    onSessionClick?.({ id: item.segment.session_id, source: item.segment.source });
  }

  return (
    <div className="stack worktime">
      <p className="panel-note">{SESSION_ENTRY_COPY.workTimelineBanner}</p>
      <section className="panel worktime-head">
        <div className="worktime-day-nav">
          <button
            type="button"
            className="date-nav-btn"
            onClick={() => setDay((current) => shiftDay(current, -1))}
            aria-label="前一天"
          >
            <Icon name="chevron" size={13} />
          </button>
          <DatePicker ariaLabel="选择日期" value={day} onChange={setDay} />
          <button
            type="button"
            className="date-nav-btn"
            onClick={() => setDay((current) => shiftDay(current, 1))}
            aria-label="后一天"
          >
            <Icon name="chevron" size={13} className="flip" />
          </button>
        </div>
        <div className="kpi-row worktime-kpis">
          <KpiCard
            icon="tokens"
            tone="purple"
            label="当日 Token 总消耗"
            value={formatCompact(data?.total_tokens ?? 0)}
          />
          <KpiCard
            icon="sessions"
            tone="cyan"
            label="工作片段数"
            value={formatTokens(data?.segment_count ?? 0)}
          />
          <KpiCard
            icon="chat"
            tone="orange"
            label="对话轮次"
            value={formatTokens(data?.turn_count ?? 0)}
          />
          <KpiCard
            icon="clock"
            tone="blue"
            label="累计 AI 执行时长"
            value={formatHoursMinutes(data?.ai_exec_minutes ?? 0)}
          />
          <KpiCard
            icon="daily"
            tone="purple"
            label="峰值并行"
            value={formatTokens(data?.peak_parallel ?? 0)}
          />
          <KpiCard
            icon="trend"
            tone="cyan"
            label="并行强度"
            value={
              data?.parallel_intensity != null ? `${formatRatio(data.parallel_intensity)}x` : "—"
            }
          />
        </div>
      </section>

      <LoadingOverlay active={loading} className="panel worktime-timeline">
        {error ? (
          <EmptyState icon="alertTriangle" tone="warn" title="加载失败" hint={error} />
        ) : segments.length === 0 && !loading ? (
          <EmptyState
            icon="clock"
            title="这天没有工作记录"
            hint="换一天试试，或检查数据源是否已同步"
          />
        ) : (
          <div className="worktime-axis-wrap">
            <div className="worktime-axis">
              {AXIS_HOURS.map((hour) => (
                <span key={hour} style={{ left: `${(hour / 24) * 100}%` }}>
                  {String(hour).padStart(2, "0")}:00
                </span>
              ))}
            </div>
            <div className="worktime-lanes" style={{ height: lanes * LANE_HEIGHT }}>
              {AXIS_HOURS.slice(1, -1).map((hour) => (
                <div
                  key={hour}
                  className="worktime-gridline"
                  style={{ left: `${(hour / 24) * 100}%` }}
                />
              ))}
              {layout.map((item) => {
                const left = (item.startMinutes / 1440) * 100;
                const width = Math.max(0, ((item.endMinutes - item.startMinutes) / 1440) * 100);
                const label = segmentLabel(item.segment);
                return (
                  <button
                    key={`${item.segment.source}:${item.segment.session_id}`}
                    type="button"
                    className="worktime-segment"
                    style={{
                      left: `${left}%`,
                      width: `${width}%`,
                      top: item.lane * LANE_HEIGHT,
                    }}
                    onClick={() => handleSegmentClick(item)}
                    onMouseEnter={(e) => handleSegmentEnter(e, item)}
                    onMouseMove={handleSegmentMove}
                    onMouseLeave={handleSegmentLeave}
                    aria-label={`${label}，点击${SESSION_ENTRY_COPY.openConversationRow}`}
                  >
                    <span>{label}</span>
                  </button>
                );
              })}
            </div>
          </div>
        )}
      </LoadingOverlay>

      {hovered && tooltipPos ? <SegmentTooltip segment={hovered} pos={tooltipPos} /> : null}
    </div>
  );
}

/// 跟随鼠标的明细气泡。不复用 `useAnchoredPanel`——那个 hook 按触发器元素定位，
/// 适合点击展开的浮层；这里需要跟随光标移动，且只在 hover 期间存在，不涉及
/// 点击外部关闭或滚动重定位（鼠标离开横条即消失）。
function SegmentTooltip({ segment, pos }: { segment: WorkSegment; pos: { x: number; y: number } }) {
  const duration = formatDuration(segment.start, segment.end);
  const label = segmentLabel(segment);
  // 气泡默认在鼠标右下方，靠近视口边缘时翻到对侧。
  const flipX = pos.x + TOOLTIP_MARGIN + TOOLTIP_WIDTH > window.innerWidth;
  const flipY = pos.y + TOOLTIP_MARGIN + TOOLTIP_HEIGHT > window.innerHeight;
  return (
    <div
      className="worktime-tooltip"
      role="tooltip"
      style={{
        position: "fixed",
        left: flipX ? pos.x - TOOLTIP_MARGIN - TOOLTIP_WIDTH : pos.x + TOOLTIP_MARGIN,
        top: flipY ? pos.y - TOOLTIP_MARGIN - TOOLTIP_HEIGHT : pos.y + TOOLTIP_MARGIN,
      }}
    >
      <div className="worktime-tooltip-title">{label}</div>
      <dl className="worktime-tooltip-grid">
        <dt>项目</dt>
        <dd>{projectLabel(segment.project)}</dd>
        <dt>来源</dt>
        <dd>
          <SourceLabel source={segment.source} size={14} />
        </dd>
        <dt>模型</dt>
        <dd>{segment.model || "—"}</dd>
        <dt>开始</dt>
        <dd>{formatClock(segment.start)}</dd>
        <dt>结束</dt>
        <dd>{formatClock(segment.end)}</dd>
        <dt>时长</dt>
        <dd>{duration ?? "—"}</dd>
        <dt>Token</dt>
        <dd>{formatTokens(segment.total_tokens)}</dd>
      </dl>
    </div>
  );
}
