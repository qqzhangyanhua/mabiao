import type { ReportPeriod, ReportPeriodKind } from "../types";
import { parseDateValue, toDateValue } from "./calendar";

function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

/** 本地日历日所在周的周一 00:00。周一为一周起点，与报告周期一致。 */
export function mondayOf(date: Date): Date {
  const day = startOfDay(date);
  const fromMonday = (day.getDay() + 6) % 7;
  day.setDate(day.getDate() - fromMonday);
  return day;
}

/** offset=0 对应的已结束自然周周一。当前进行中的一周不可寻址。 */
export function lastCompletedWeekMonday(today = new Date()): Date {
  const monday = mondayOf(today);
  monday.setDate(monday.getDate() - 7);
  return monday;
}

export function lastCompletedWeekEnd(today = new Date()): string {
  const sunday = lastCompletedWeekMonday(today);
  sunday.setDate(sunday.getDate() + 6);
  return toDateValue(sunday);
}

export function weekStartFromOffset(offset: number, today = new Date()): string {
  const monday = lastCompletedWeekMonday(today);
  monday.setDate(monday.getDate() - Math.max(0, offset) * 7);
  return toDateValue(monday);
}

/**
 * 把日历日映射到 `get_report` 的周 offset。
 * 选中日所在周若尚未结束或在未来，落到最近一个已结束周（offset 0）。
 */
export function weekOffsetFromDate(value: string, today = new Date()): number {
  const date = parseDateValue(value);
  if (!date) {
    return 0;
  }
  const selectedMonday = mondayOf(date);
  const lastMonday = lastCompletedWeekMonday(today);
  const diffDays = Math.round((lastMonday.getTime() - selectedMonday.getTime()) / 86_400_000);
  if (diffDays <= 0) {
    return 0;
  }
  return Math.floor(diffDays / 7);
}

function firstOfMonth(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), 1);
}

export function lastCompletedMonthStart(today = new Date()): Date {
  const start = firstOfMonth(today);
  start.setMonth(start.getMonth() - 1);
  return start;
}

export function lastCompletedMonthEnd(today = new Date()): string {
  return toDateValue(new Date(today.getFullYear(), today.getMonth(), 0));
}

export function monthStartFromOffset(offset: number, today = new Date()): string {
  const start = lastCompletedMonthStart(today);
  start.setMonth(start.getMonth() - Math.max(0, offset));
  return toDateValue(start);
}

export function monthOffsetFromDate(value: string, today = new Date()): number {
  const date = parseDateValue(value);
  if (!date) {
    return 0;
  }
  const selected = firstOfMonth(date);
  const last = lastCompletedMonthStart(today);
  const months =
    (last.getFullYear() - selected.getFullYear()) * 12 + (last.getMonth() - selected.getMonth());
  return months <= 0 ? 0 : months;
}

export const CUSTOM_PERIOD_MAX_DAYS = 93;

export function todayDateValue(today = new Date()): string {
  return toDateValue(startOfDay(today));
}

export function shiftDateValue(value: string, days: number): string | null {
  const date = parseDateValue(value);
  if (!date) {
    return null;
  }
  date.setDate(date.getDate() + days);
  return toDateValue(date);
}

export function inclusiveDayCount(from: string, to: string): number | null {
  const start = parseDateValue(from);
  const end = parseDateValue(to);
  if (!start || !end) {
    return null;
  }
  return Math.round((end.getTime() - start.getTime()) / 86_400_000) + 1;
}

export function periodOffsetFromDate(
  kind: ReportPeriodKind,
  value: string,
  today = new Date(),
): number {
  if (kind === "month") {
    return monthOffsetFromDate(value, today);
  }
  if (kind === "week") {
    return weekOffsetFromDate(value, today);
  }
  return 0;
}

export function periodStartFromOffset(
  kind: ReportPeriodKind,
  offset: number,
  today = new Date(),
): string {
  if (kind === "month") {
    return monthStartFromOffset(offset, today);
  }
  if (kind === "week") {
    return weekStartFromOffset(offset, today);
  }
  return todayDateValue(today);
}

export function periodEndFromOffset(
  kind: ReportPeriodKind,
  offset: number,
  today = new Date(),
): string {
  if (kind === "month") {
    const start = lastCompletedMonthStart(today);
    return toDateValue(
      new Date(start.getFullYear(), start.getMonth() - Math.max(0, offset) + 1, 0),
    );
  }
  if (kind === "custom") {
    return todayDateValue(today);
  }
  const monday = lastCompletedWeekMonday(today);
  monday.setDate(monday.getDate() - Math.max(0, offset) * 7 + 6);
  return toDateValue(monday);
}

export function latestSelectableDate(kind: ReportPeriodKind, today = new Date()): string {
  if (kind === "month") {
    return lastCompletedMonthEnd(today);
  }
  if (kind === "custom") {
    return todayDateValue(today);
  }
  return lastCompletedWeekEnd(today);
}

export function clampCustomRange(
  from: string,
  to: string,
  today = new Date(),
  edited: "from" | "to" = "to",
): { from: string; to: string } {
  const todayValue = todayDateValue(today);
  let fromValue = parseDateValue(from) ? from : todayValue;
  let toValue = parseDateValue(to) ? to : todayValue;
  if (fromValue > todayValue) {
    fromValue = todayValue;
  }
  if (toValue > todayValue) {
    toValue = todayValue;
  }
  if (fromValue > toValue) {
    const swap = fromValue;
    fromValue = toValue;
    toValue = swap;
  }
  const count = inclusiveDayCount(fromValue, toValue) ?? 1;
  if (count <= CUSTOM_PERIOD_MAX_DAYS) {
    return { from: fromValue, to: toValue };
  }
  if (edited === "from") {
    fromValue = shiftDateValue(toValue, -(CUSTOM_PERIOD_MAX_DAYS - 1)) ?? fromValue;
    return { from: fromValue, to: toValue };
  }
  let nextTo = shiftDateValue(fromValue, CUSTOM_PERIOD_MAX_DAYS - 1) ?? toValue;
  if (nextTo > todayValue) {
    nextTo = todayValue;
    fromValue = shiftDateValue(nextTo, -(CUSTOM_PERIOD_MAX_DAYS - 1)) ?? fromValue;
  }
  return { from: fromValue, to: nextTo };
}

function earlierDate(a: string, b: string): string {
  return a < b ? a : b;
}

export function customPickerBounds(
  from: string,
  to: string,
  today = new Date(),
): { fromMin: string; fromMax: string; toMin: string; toMax: string } {
  const todayValue = todayDateValue(today);
  const earliestFrom = shiftDateValue(to, -(CUSTOM_PERIOD_MAX_DAYS - 1)) ?? to;
  const latestTo = shiftDateValue(from, CUSTOM_PERIOD_MAX_DAYS - 1) ?? from;
  return {
    fromMin: earliestFrom,
    fromMax: earlierDate(to, todayValue),
    toMin: from,
    toMax: earlierDate(latestTo, todayValue),
  };
}

export function reportPeriodPayload(
  kind: ReportPeriodKind,
  offset: number,
  customFrom: string,
  customTo: string,
): ReportPeriod {
  if (kind === "custom") {
    return { kind, offset: 0, from: customFrom, to: customTo };
  }
  return { kind, offset };
}
