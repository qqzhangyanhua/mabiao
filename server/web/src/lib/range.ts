export type RangePreset = "7d" | "30d" | "month" | "custom";

/** 日期区间，两端含，`YYYY-MM-DD`，按浏览器所在时区的日历。 */
export interface DayRange {
  fromDay: string;
  toDay: string;
}

const DAY_MS = 86_400_000;
const MINUTE_MS = 60_000;

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

/** 把「偏移后的时间」当 UTC 读出日历日。 */
function dayOf(shifted: Date): string {
  return `${shifted.getUTCFullYear()}-${pad(shifted.getUTCMonth() + 1)}-${pad(shifted.getUTCDate())}`;
}

/** 浏览器当前的 UTC 偏移（分钟，东为正），与服务端 `tz_offset_minutes` 同一符号。 */
export function localOffsetMinutes(now: Date = new Date()): number {
  return -now.getTimezoneOffset();
}

export function presetRange(
  preset: Exclude<RangePreset, "custom">,
  now: Date,
  offsetMinutes: number,
): DayRange {
  const shifted = new Date(now.getTime() + offsetMinutes * MINUTE_MS);
  const toDay = dayOf(shifted);
  if (preset === "month") {
    return { fromDay: `${shifted.getUTCFullYear()}-${pad(shifted.getUTCMonth() + 1)}-01`, toDay };
  }
  const days = preset === "7d" ? 7 : 30;
  return { fromDay: dayOf(new Date(shifted.getTime() - (days - 1) * DAY_MS)), toDay };
}

function dayStartUtcMs(day: string, offsetMinutes: number): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!match) return null;
  const [, y, m, d] = match;
  const ms = Date.UTC(Number(y), Number(m) - 1, Number(d));
  return Number.isNaN(ms) ? null : ms - offsetMinutes * MINUTE_MS;
}

export interface QueryBounds {
  /** 含。 */
  from: string;
  /** 不含：结束日的次日 0 点。 */
  to: string;
}

/** 日期区间 → 服务端的 `from` / `to`。日期格式不对或起止颠倒返回 `null`。 */
export function queryBounds(range: DayRange, offsetMinutes: number): QueryBounds | null {
  const from = dayStartUtcMs(range.fromDay, offsetMinutes);
  const to = dayStartUtcMs(range.toDay, offsetMinutes);
  if (from === null || to === null || to < from) return null;
  return { from: new Date(from).toISOString(), to: new Date(to + DAY_MS).toISOString() };
}
