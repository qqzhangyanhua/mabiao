import type { WorkNotesRange, WorkNotesRangeKind } from "../types";
import { parseDateValue, toDateValue } from "./calendar";
import { inclusiveDayCount, mondayOf, shiftDateValue, todayDateValue } from "./reportPeriod";

export { todayDateValue };

export const WORK_NOTES_CUSTOM_MAX_DAYS = 31;

export function thisWeekStartDate(today = new Date()): string {
  return toDateValue(mondayOf(today));
}

export function thisMonthStartDate(today = new Date()): string {
  return toDateValue(new Date(today.getFullYear(), today.getMonth(), 1));
}

export function clampWorkNotesCustomRange(
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
  if (count <= WORK_NOTES_CUSTOM_MAX_DAYS) {
    return { from: fromValue, to: toValue };
  }
  if (edited === "from") {
    fromValue = shiftDateValue(toValue, -(WORK_NOTES_CUSTOM_MAX_DAYS - 1)) ?? fromValue;
    return { from: fromValue, to: toValue };
  }
  let nextTo = shiftDateValue(fromValue, WORK_NOTES_CUSTOM_MAX_DAYS - 1) ?? toValue;
  if (nextTo > todayValue) {
    nextTo = todayValue;
    fromValue = shiftDateValue(nextTo, -(WORK_NOTES_CUSTOM_MAX_DAYS - 1)) ?? fromValue;
  }
  return { from: fromValue, to: nextTo };
}

function earlierDate(a: string, b: string): string {
  return a < b ? a : b;
}

export function workNotesCustomPickerBounds(
  from: string,
  to: string,
  today = new Date(),
): { fromMin: string; fromMax: string; toMin: string; toMax: string } {
  const todayValue = todayDateValue(today);
  const earliestFrom = shiftDateValue(to, -(WORK_NOTES_CUSTOM_MAX_DAYS - 1)) ?? to;
  const latestTo = shiftDateValue(from, WORK_NOTES_CUSTOM_MAX_DAYS - 1) ?? from;
  return {
    fromMin: earliestFrom,
    fromMax: earlierDate(to, todayValue),
    toMin: from,
    toMax: earlierDate(latestTo, todayValue),
  };
}

export function workNotesRangePayload(
  kind: WorkNotesRangeKind,
  customFrom: string,
  customTo: string,
): WorkNotesRange {
  if (kind === "custom") {
    return { kind, from: customFrom, to: customTo };
  }
  return { kind };
}

export function workNotesRangeKindLabel(kind: WorkNotesRangeKind): string {
  if (kind === "today") {
    return "今天";
  }
  if (kind === "this_month") {
    return "本月";
  }
  if (kind === "custom") {
    return "区间";
  }
  return "本周";
}

export function workNotesHistoryDateLabel(start: string, end: string): string {
  const from = parseDateValue(start);
  const to = parseDateValue(end);
  if (!from || !to) {
    return start === end ? start : `${start} 至 ${end}`;
  }
  const fromText = `${from.getMonth() + 1}月${from.getDate()}日`;
  const toText = `${to.getMonth() + 1}月${to.getDate()}日`;
  if (start === end) {
    return fromText;
  }
  if (from.getFullYear() !== to.getFullYear()) {
    return `${from.getFullYear()}年${fromText} – ${to.getFullYear()}年${toText}`;
  }
  return `${fromText} – ${toText}`;
}

export function workNotesRangeCopy(kind: WorkNotesRangeKind): {
  help: string;
  emptyHint: string;
} {
  if (kind === "today") {
    return {
      help: "今天 00:00 到此刻。",
      emptyHint: "换一段时间再试，或先去对话记录确认有正文。",
    };
  }
  if (kind === "this_month") {
    return {
      help: "本月 1 号 00:00 到此刻。",
      emptyHint: "换一段时间再试，或先去对话记录确认有正文。",
    };
  }
  if (kind === "custom") {
    return {
      help: `自选起止日，含首尾两天，最长 ${WORK_NOTES_CUSTOM_MAX_DAYS} 天，不能选到未来。`,
      emptyHint: "可以改起止日期，或先去对话记录确认有正文。",
    };
  }
  return {
    help: "本周一 00:00 到此刻。",
    emptyHint: "选一段时间后点生成。区间内的对话会交给本机 Codex 总结。",
  };
}
