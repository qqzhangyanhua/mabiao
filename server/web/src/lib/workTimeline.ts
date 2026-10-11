import type { WorkSegment } from "../api/types";

export const DAY_MINUTES = 24 * 60;
/** 极短/零时长的会话仍给这么多分钟宽度，保证在时间轴上可见、可点击。 */
export const MIN_SEGMENT_MINUTES = 6;

export type LaneSegment = {
  segment: WorkSegment;
  startMinutes: number;
  endMinutes: number;
  lane: number;
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** 把一个 ISO 时刻换算成距 `dayStartIso` 的分钟数，钳制在 [0, 1440]。 */
export function minutesSinceDayStart(iso: string, dayStartIso: string): number {
  const start = Date.parse(dayStartIso);
  const at = Date.parse(iso);
  if (Number.isNaN(start) || Number.isNaN(at)) {
    return 0;
  }
  return clamp(Math.round((at - start) / 60_000), 0, DAY_MINUTES);
}

/**
 * 泳道贪心布局：按开始时间排序后，把每个片段放进第一条「结束时间不晚于当前片段开始时间」
 * 的已有泳道；找不到就新开一条。撑宽只影响布局，不改片段本身的时间/token。
 */
export function layoutSegments(segments: WorkSegment[], dayStartIso: string): LaneSegment[] {
  const withMinutes = segments
    .map((segment) => {
      const startMinutes = minutesSinceDayStart(segment.start, dayStartIso);
      const rawEnd = minutesSinceDayStart(segment.end, dayStartIso);
      const endMinutes = Math.min(DAY_MINUTES, Math.max(rawEnd, startMinutes + MIN_SEGMENT_MINUTES));
      return { segment, startMinutes, endMinutes };
    })
    .sort((a, b) => a.startMinutes - b.startMinutes || a.endMinutes - b.endMinutes);

  const laneEnds: number[] = [];
  const placed: LaneSegment[] = [];
  for (const item of withMinutes) {
    let lane = laneEnds.findIndex((end) => end <= item.startMinutes);
    if (lane === -1) {
      lane = laneEnds.length;
      laneEnds.push(item.endMinutes);
    } else {
      laneEnds[lane] = item.endMinutes;
    }
    placed.push({ ...item, lane });
  }
  return placed;
}

export function laneCount(layout: LaneSegment[]): number {
  return layout.reduce((max, item) => Math.max(max, item.lane + 1), 0);
}

/** 所选日历日零点对应的 ISO 时刻（按调用方所在时区）。 */
export function dayStartIso(day: string): string {
  return new Date(`${day}T00:00:00`).toISOString();
}

export function formatClock(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "—";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function formatDuration(start: string, end: string): string {
  const ms = Date.parse(end) - Date.parse(start);
  if (!Number.isFinite(ms) || ms < 0) return "—";
  const minutes = Math.round(ms / 60_000);
  if (minutes < 1) return "不足 1 分钟";
  if (minutes < 60) return `${minutes} 分钟`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours} 小时` : `${hours} 小时 ${rest} 分钟`;
}

export function weekdayLabel(day: string): string {
  const date = new Date(`${day}T00:00:00`);
  if (Number.isNaN(date.getTime())) return "";
  return ["日", "一", "二", "三", "四", "五", "六"][date.getDay()] ?? "";
}
