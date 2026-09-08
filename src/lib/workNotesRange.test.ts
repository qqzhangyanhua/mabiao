import { describe, expect, it } from "vitest";
import {
  clampWorkNotesCustomRange,
  thisMonthStartDate,
  thisWeekStartDate,
  WORK_NOTES_CUSTOM_MAX_DAYS,
  workNotesCustomPickerBounds,
  workNotesHistoryDateLabel,
  workNotesRangeCopy,
  workNotesRangeKindLabel,
  workNotesRangePayload,
} from "./workNotesRange";

/** 2026-08-19 周三：本周一 8/17，本月 1 号 8/01。 */
const TODAY = new Date(2026, 7, 19);

describe("thisWeekStartDate / thisMonthStartDate", () => {
  it("uses the in-progress week and month, not the last completed period", () => {
    expect(thisWeekStartDate(TODAY)).toBe("2026-08-17");
    expect(thisMonthStartDate(TODAY)).toBe("2026-08-01");
  });
});

describe("workNotesRangeCopy", () => {
  it("describes until-now semantics, not a completed natural period", () => {
    expect(workNotesRangeCopy("today")).toEqual({
      help: "今天 00:00 到此刻。",
      emptyHint: "换一段时间再试，或先去对话记录确认有正文。",
    });
    expect(workNotesRangeCopy("this_week")).toEqual({
      help: "本周一 00:00 到此刻。",
      emptyHint: "选一段时间后点生成。区间内的对话会交给本机 Codex 总结。",
    });
    expect(workNotesRangeCopy("this_month")).toEqual({
      help: "本月 1 号 00:00 到此刻。",
      emptyHint: "换一段时间再试，或先去对话记录确认有正文。",
    });
    expect(workNotesRangeCopy("custom")).toEqual({
      help: "自选起止日，含首尾两天，最长 31 天，不能选到未来。",
      emptyHint: "可以改起止日期，或先去对话记录确认有正文。",
    });
  });
});

describe("clampWorkNotesCustomRange", () => {
  it("keeps an inclusive range inside 31 days", () => {
    expect(clampWorkNotesCustomRange("2026-08-01", "2026-08-13", TODAY)).toEqual({
      from: "2026-08-01",
      to: "2026-08-13",
    });
  });

  it("clamps the end to today and swaps inverted bounds", () => {
    expect(clampWorkNotesCustomRange("2026-08-18", "2026-08-25", TODAY)).toEqual({
      from: "2026-08-18",
      to: "2026-08-19",
    });
    expect(clampWorkNotesCustomRange("2026-08-13", "2026-08-01", TODAY)).toEqual({
      from: "2026-08-01",
      to: "2026-08-13",
    });
  });

  it("caps the span at 31 days from the edited bound", () => {
    expect(WORK_NOTES_CUSTOM_MAX_DAYS).toBe(31);
    expect(clampWorkNotesCustomRange("2026-07-01", "2026-08-19", TODAY, "to")).toEqual({
      from: "2026-07-01",
      to: "2026-07-31",
    });
    expect(clampWorkNotesCustomRange("2026-07-01", "2026-08-19", TODAY, "from")).toEqual({
      from: "2026-07-20",
      to: "2026-08-19",
    });
  });
});

describe("workNotesCustomPickerBounds", () => {
  it("keeps both pickers inside today and the 31-day window", () => {
    expect(workNotesCustomPickerBounds("2026-08-01", "2026-08-13", TODAY)).toEqual({
      fromMin: "2026-07-14",
      fromMax: "2026-08-13",
      toMin: "2026-08-01",
      toMax: "2026-08-19",
    });
  });
});

describe("workNotesRangeKindLabel / workNotesHistoryDateLabel", () => {
  it("names the four range kinds", () => {
    expect(workNotesRangeKindLabel("today")).toBe("今天");
    expect(workNotesRangeKindLabel("this_week")).toBe("本周");
    expect(workNotesRangeKindLabel("this_month")).toBe("本月");
    expect(workNotesRangeKindLabel("custom")).toBe("区间");
  });

  it("collapses a single day and drops the year inside the same year", () => {
    expect(workNotesHistoryDateLabel("2026-08-19", "2026-08-19")).toBe("8月19日");
    expect(workNotesHistoryDateLabel("2026-08-17", "2026-08-19")).toBe("8月17日 – 8月19日");
    expect(workNotesHistoryDateLabel("2025-12-31", "2026-01-02")).toBe(
      "2025年12月31日 – 2026年1月2日",
    );
  });
});

describe("workNotesRangePayload", () => {
  it("sends from/to only for custom", () => {
    expect(workNotesRangePayload("today", "2026-08-01", "2026-08-13")).toEqual({
      kind: "today",
    });
    expect(workNotesRangePayload("this_week", "2026-08-01", "2026-08-13")).toEqual({
      kind: "this_week",
    });
    expect(workNotesRangePayload("this_month", "2026-08-01", "2026-08-13")).toEqual({
      kind: "this_month",
    });
    expect(workNotesRangePayload("custom", "2026-08-01", "2026-08-13")).toEqual({
      kind: "custom",
      from: "2026-08-01",
      to: "2026-08-13",
    });
  });
});
