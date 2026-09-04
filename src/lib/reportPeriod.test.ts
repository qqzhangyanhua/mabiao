import { describe, expect, it } from "vitest";
import {
  clampCustomRange,
  CUSTOM_PERIOD_MAX_DAYS,
  customPickerBounds,
  inclusiveDayCount,
  lastCompletedMonthEnd,
  lastCompletedMonthStart,
  lastCompletedWeekEnd,
  lastCompletedWeekMonday,
  latestSelectableDate,
  mondayOf,
  monthOffsetFromDate,
  monthStartFromOffset,
  periodEndFromOffset,
  periodOffsetFromDate,
  reportPeriodPayload,
  shiftDateValue,
  weekOffsetFromDate,
  weekStartFromOffset,
} from "./reportPeriod";
import { toDateValue } from "./calendar";

/** 2026-09-04 周五：本周周一 8/31，最近已结束周 8/24–8/30。 */
const TODAY = new Date(2026, 8, 4);

describe("mondayOf", () => {
  it("returns Monday for a Wednesday", () => {
    expect(toDateValue(mondayOf(new Date(2026, 7, 26)))).toBe("2026-08-24");
  });

  it("keeps Monday as Monday", () => {
    expect(toDateValue(mondayOf(new Date(2026, 7, 24)))).toBe("2026-08-24");
  });

  it("maps Sunday back to the preceding Monday", () => {
    expect(toDateValue(mondayOf(new Date(2026, 7, 30)))).toBe("2026-08-24");
  });
});

describe("lastCompletedWeek", () => {
  it("uses the week before the current in-progress week", () => {
    expect(toDateValue(lastCompletedWeekMonday(TODAY))).toBe("2026-08-24");
    expect(lastCompletedWeekEnd(TODAY)).toBe("2026-08-30");
  });
});

describe("weekOffsetFromDate", () => {
  it("maps any day in the last completed week to offset 0", () => {
    expect(weekOffsetFromDate("2026-08-24", TODAY)).toBe(0);
    expect(weekOffsetFromDate("2026-08-26", TODAY)).toBe(0);
    expect(weekOffsetFromDate("2026-08-30", TODAY)).toBe(0);
  });

  it("counts completed weeks backward", () => {
    expect(weekOffsetFromDate("2026-08-17", TODAY)).toBe(1);
    expect(weekOffsetFromDate("2026-08-10", TODAY)).toBe(2);
  });

  it("clamps the current week and future dates to offset 0", () => {
    expect(weekOffsetFromDate("2026-09-02", TODAY)).toBe(0);
    expect(weekOffsetFromDate("2026-09-10", TODAY)).toBe(0);
  });

  it("falls back to 0 for malformed dates", () => {
    expect(weekOffsetFromDate("not-a-date", TODAY)).toBe(0);
  });
});

describe("weekStartFromOffset", () => {
  it("returns the Monday of the addressed completed week", () => {
    expect(weekStartFromOffset(0, TODAY)).toBe("2026-08-24");
    expect(weekStartFromOffset(1, TODAY)).toBe("2026-08-17");
  });
});

describe("month period", () => {
  it("uses the calendar month before the current in-progress month", () => {
    expect(toDateValue(lastCompletedMonthStart(TODAY))).toBe("2026-08-01");
    expect(lastCompletedMonthEnd(TODAY)).toBe("2026-08-31");
  });

  it("maps any day in the last completed month to offset 0", () => {
    expect(monthOffsetFromDate("2026-08-01", TODAY)).toBe(0);
    expect(monthOffsetFromDate("2026-08-15", TODAY)).toBe(0);
    expect(monthOffsetFromDate("2026-08-31", TODAY)).toBe(0);
  });

  it("counts completed months backward", () => {
    expect(monthOffsetFromDate("2026-07-20", TODAY)).toBe(1);
    expect(monthOffsetFromDate("2026-06-01", TODAY)).toBe(2);
  });

  it("clamps the current month and future dates to offset 0", () => {
    expect(monthOffsetFromDate("2026-09-02", TODAY)).toBe(0);
    expect(monthOffsetFromDate("2026-10-01", TODAY)).toBe(0);
  });

  it("returns the first of the addressed completed month", () => {
    expect(monthStartFromOffset(0, TODAY)).toBe("2026-08-01");
    expect(monthStartFromOffset(1, TODAY)).toBe("2026-07-01");
  });
});

describe("periodOffsetFromDate", () => {
  it("dispatches week and month", () => {
    expect(periodOffsetFromDate("week", "2026-08-10", TODAY)).toBe(2);
    expect(periodOffsetFromDate("month", "2026-07-04", TODAY)).toBe(1);
    expect(periodOffsetFromDate("custom", "2026-09-01", TODAY)).toBe(0);
    expect(latestSelectableDate("week", TODAY)).toBe("2026-08-30");
    expect(latestSelectableDate("month", TODAY)).toBe("2026-08-31");
    expect(latestSelectableDate("custom", TODAY)).toBe("2026-09-04");
  });
});

describe("periodEndFromOffset", () => {
  it("returns the last day of the addressed completed week or month", () => {
    expect(periodEndFromOffset("week", 0, TODAY)).toBe("2026-08-30");
    expect(periodEndFromOffset("week", 1, TODAY)).toBe("2026-08-23");
    expect(periodEndFromOffset("month", 0, TODAY)).toBe("2026-08-31");
    expect(periodEndFromOffset("month", 1, TODAY)).toBe("2026-07-31");
    expect(periodEndFromOffset("custom", 0, TODAY)).toBe("2026-09-04");
  });
});

describe("clampCustomRange", () => {
  it("keeps an inclusive in-month range such as 1-13 of last month", () => {
    expect(clampCustomRange("2026-08-01", "2026-08-13", TODAY)).toEqual({
      from: "2026-08-01",
      to: "2026-08-13",
    });
    expect(inclusiveDayCount("2026-08-01", "2026-08-13")).toBe(13);
  });

  it("clamps the end to today and swaps inverted bounds", () => {
    expect(clampCustomRange("2026-09-01", "2026-09-13", TODAY)).toEqual({
      from: "2026-09-01",
      to: "2026-09-04",
    });
    expect(clampCustomRange("2026-08-13", "2026-08-01", TODAY)).toEqual({
      from: "2026-08-01",
      to: "2026-08-13",
    });
  });

  it("caps the span at 93 days from the edited bound", () => {
    expect(clampCustomRange("2026-05-01", "2026-09-04", TODAY, "to")).toEqual({
      from: "2026-05-01",
      to: shiftDateValue("2026-05-01", CUSTOM_PERIOD_MAX_DAYS - 1),
    });
    expect(clampCustomRange("2026-05-01", "2026-09-04", TODAY, "from")).toEqual({
      from: shiftDateValue("2026-09-04", -(CUSTOM_PERIOD_MAX_DAYS - 1)),
      to: "2026-09-04",
    });
  });
});

describe("customPickerBounds", () => {
  it("keeps both pickers inside today and the 93-day window", () => {
    expect(customPickerBounds("2026-08-01", "2026-08-13", TODAY)).toEqual({
      fromMin: "2026-05-13",
      fromMax: "2026-08-13",
      toMin: "2026-08-01",
      toMax: "2026-09-04",
    });
  });
});

describe("reportPeriodPayload", () => {
  it("sends from/to only for custom", () => {
    expect(reportPeriodPayload("week", 2, "2026-08-01", "2026-08-13")).toEqual({
      kind: "week",
      offset: 2,
    });
    expect(reportPeriodPayload("custom", 9, "2026-08-01", "2026-08-13")).toEqual({
      kind: "custom",
      offset: 0,
      from: "2026-08-01",
      to: "2026-08-13",
    });
  });
});
