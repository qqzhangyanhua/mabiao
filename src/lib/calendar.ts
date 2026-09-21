import type { Filter, Grain } from "../types";

const WEEKDAYS = ["一", "二", "三", "四", "五", "六", "日"] as const;
const HEATMAP_WEEKS = 53;

export function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

export function toDateValue(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

export function parseDateValue(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) {
    return null;
  }
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const date = new Date(year, month - 1, day);
  if (date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) {
    return null;
  }
  return date;
}

export function formatDateLabel(value: string): string {
  const date = parseDateValue(value);
  if (!date) {
    return "选择日期";
  }
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

export function monthTitle(year: number, month: number): string {
  return `${year} 年 ${month + 1} 月`;
}

export function weekdayLabels(): readonly string[] {
  return WEEKDAYS;
}

/** 生成当月日历格子，周一为一周起点；前后月补齐。 */
export function calendarCells(year: number, month: number): { value: string; inMonth: boolean }[] {
  const first = new Date(year, month, 1);
  const startOffset = (first.getDay() + 6) % 7;
  const start = new Date(year, month, 1 - startOffset);
  return Array.from({ length: 42 }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    return {
      value: toDateValue(date),
      inMonth: date.getMonth() === month,
    };
  });
}

export function shiftMonth(
  year: number,
  month: number,
  delta: number,
): { year: number; month: number } {
  const next = new Date(year, month + delta, 1);
  return { year: next.getFullYear(), month: next.getMonth() };
}

export type HeatmapWindow = {
  from: string;
  to: string;
  fromDate: string;
  toDate: string;
};

/** 以 end 所在周为最后一列，往前取 53 周（周一起始），返回查询用 ISO 与日历用日期。 */
export function heatmapWindow(end: Date): HeatmapWindow {
  const safeEnd = Number.isNaN(end.getTime()) ? new Date() : end;
  const endDay = new Date(safeEnd.getFullYear(), safeEnd.getMonth(), safeEnd.getDate());
  const mondayOffset = (endDay.getDay() + 6) % 7;
  const thisMonday = new Date(endDay);
  thisMonday.setDate(endDay.getDate() - mondayOffset);
  const startMonday = new Date(thisMonday);
  startMonday.setDate(thisMonday.getDate() - (HEATMAP_WEEKS - 1) * 7);
  const fromDate = toDateValue(startMonday);
  const toDate = toDateValue(endDay);
  return {
    from: new Date(`${fromDate}T00:00:00`).toISOString(),
    to: new Date(`${toDate}T23:59:59.999`).toISOString(),
    fromDate,
    toDate,
  };
}

/** 保留来源/模型/项目/Provider 筛选，日期窗口固定为近 53 周。 */
export function heatmapFilter(filter: Filter): {
  filter: Filter;
  fromDate: string;
  toDate: string;
} {
  const parsed = filter.to ? new Date(filter.to) : new Date();
  const window = heatmapWindow(parsed);
  return {
    filter: {
      ...filter,
      from: window.from,
      to: window.to,
    },
    fromDate: window.fromDate,
    toDate: window.toDate,
  };
}

export type HeatmapCell = {
  date: string;
  future: boolean;
};

export type HeatmapWeek = {
  days: HeatmapCell[];
};

export type HeatmapMonthLabel = {
  label: string;
  weekIndex: number;
};

/** 从 from 所在周一起到 to 所在周日，按列（一周）排出 7 行；to 之后标 future。 */
export function heatmapGrid(fromDate: string, toDate: string): HeatmapWeek[] {
  const start = parseDateValue(fromDate);
  const end = parseDateValue(toDate);
  if (!start || !end || start.getTime() > end.getTime()) {
    return [];
  }
  const mondayOffset = (start.getDay() + 6) % 7;
  const cursor = new Date(start);
  cursor.setDate(start.getDate() - mondayOffset);
  const endMondayOffset = (end.getDay() + 6) % 7;
  const lastSunday = new Date(end);
  lastSunday.setDate(end.getDate() + (6 - endMondayOffset));

  const weeks: HeatmapWeek[] = [];
  while (cursor.getTime() <= lastSunday.getTime()) {
    const days: HeatmapCell[] = [];
    for (let index = 0; index < 7; index += 1) {
      days.push({
        date: toDateValue(cursor),
        future: cursor.getTime() > end.getTime(),
      });
      cursor.setDate(cursor.getDate() + 1);
    }
    weeks.push({ days });
  }
  return weeks;
}

/** 含该月 1 号的周才标月份；首列补起始月；相邻列距小于 2 则跳过。 */
export function heatmapMonthLabels(weeks: HeatmapWeek[]): HeatmapMonthLabel[] {
  const candidates: HeatmapMonthLabel[] = [];
  for (let weekIndex = 0; weekIndex < weeks.length; weekIndex += 1) {
    const week = weeks[weekIndex];
    if (!week) {
      continue;
    }
    const firstOfMonth = week.days.find((cell) => {
      if (cell.future) {
        return false;
      }
      const date = parseDateValue(cell.date);
      return date !== null && date.getDate() === 1;
    });
    if (!firstOfMonth) {
      continue;
    }
    const date = parseDateValue(firstOfMonth.date);
    if (!date) {
      continue;
    }
    candidates.push({ label: `${date.getMonth() + 1}月`, weekIndex });
  }

  const firstDay = weeks[0]?.days.find((cell) => !cell.future);
  if (firstDay && candidates[0]?.weekIndex !== 0) {
    const date = parseDateValue(firstDay.date);
    if (date) {
      candidates.unshift({ label: `${date.getMonth() + 1}月`, weekIndex: 0 });
    }
  }

  const labels: HeatmapMonthLabel[] = [];
  for (const item of candidates) {
    const prev = labels[labels.length - 1];
    if (prev && item.weekIndex - prev.weekIndex < 2) {
      continue;
    }
    labels.push(item);
  }
  return labels;
}

export function quantileCuts(values: number[]): number[] {
  if (values.length === 0) {
    return [];
  }
  const sorted = [...values].sort((a, b) => a - b);
  const at = (p: number) =>
    sorted[Math.min(sorted.length - 1, Math.round(p * (sorted.length - 1)))];
  return [...new Set([at(0.25), at(0.5), at(0.75), sorted[sorted.length - 1]])].sort(
    (a, b) => a - b,
  );
}

/** 0 为空档，1–4 为非零分位（与旧 visualMap 分档一致）。 */
export function tokenHeatmapLevel(value: number, cuts: number[]): number {
  if (value <= 0) {
    return 0;
  }
  let prev = 0;
  let level = 0;
  for (const upper of cuts) {
    if (upper <= prev) {
      continue;
    }
    level += 1;
    if (value <= upper) {
      return Math.min(level, 4);
    }
    prev = upper;
  }
  return Math.min(Math.max(level, 1), 4);
}

export type HeatmapTooltipBox = {
  left: number;
  top: number;
  right: number;
  bottom: number;
};

/** 相对格子水平居中、优先开在上方；贴边时改对齐，避免视口裁切。 */
export function placeHeatmapTooltip(
  cell: HeatmapTooltipBox,
  tip: { width: number; height: number },
  viewport: { width: number; height: number },
  pad = 8,
  gap = 8,
): { left: number; top: number } {
  const width = Math.max(0, tip.width);
  const height = Math.max(0, tip.height);
  const viewW = Math.max(0, viewport.width);
  const viewH = Math.max(0, viewport.height);
  const cellCenter = (cell.left + cell.right) / 2;
  const maxLeft = Math.max(pad, viewW - width - pad);
  const left = Math.min(Math.max(pad, cellCenter - width / 2), maxLeft);
  const above = cell.top - gap - height;
  const top =
    above >= pad ? above : Math.min(cell.bottom + gap, Math.max(pad, viewH - height - pad));
  return { left, top };
}

/**
 * 把趋势 bucket（与后端 `strftime` / `substr` 口径一致）换成自定义区间用的起止日。
 * hour/day 落到当天；week 为该 ISO 周周一至周日；month 为该月 1 号至月末。
 */
export function bucketToDateRange(
  grain: Grain,
  bucket: string,
): { from: string; to: string } | null {
  if (grain === "hour") {
    const match = /^(\d{4}-\d{2}-\d{2})T(\d{2})$/.exec(bucket);
    if (!match) {
      return null;
    }
    const day = match[1];
    const hour = Number(match[2]);
    if (hour > 23 || !parseDateValue(day)) {
      return null;
    }
    return { from: day, to: day };
  }
  if (grain === "day") {
    if (!parseDateValue(bucket)) {
      return null;
    }
    return { from: bucket, to: bucket };
  }
  if (grain === "week") {
    const match = /^(\d{4})-W(\d{2})$/.exec(bucket);
    if (!match) {
      return null;
    }
    const week = Number(match[2]);
    if (week < 1 || week > 53) {
      return null;
    }
    return isoWeekDateRange(Number(match[1]), week);
  }
  const match = /^(\d{4})-(\d{2})$/.exec(bucket);
  if (!match) {
    return null;
  }
  const year = Number(match[1]);
  const month = Number(match[2]);
  if (month < 1 || month > 12) {
    return null;
  }
  const last = new Date(year, month, 0);
  return { from: `${year}-${pad2(month)}-01`, to: toDateValue(last) };
}

/** ISO 周年的第 1 周包含 1 月 4 日；周一起始。 */
function isoWeekDateRange(isoYear: number, week: number): { from: string; to: string } {
  const jan4 = new Date(isoYear, 0, 4);
  const jan4Dow = jan4.getDay() || 7;
  const week1Monday = new Date(isoYear, 0, 4 - (jan4Dow - 1));
  const monday = new Date(week1Monday);
  monday.setDate(week1Monday.getDate() + (week - 1) * 7);
  const sunday = new Date(monday);
  sunday.setDate(monday.getDate() + 6);
  return { from: toDateValue(monday), to: toDateValue(sunday) };
}
