import { describe, expect, it } from "vitest";
import {
  bucketToDateRange,
  calendarCells,
  formatDateLabel,
  heatmapGrid,
  heatmapMonthLabels,
  heatmapWindow,
  monthTitle,
  pad2,
  parseDateValue,
  placeHeatmapTooltip,
  quantileCuts,
  shiftMonth,
  toDateValue,
  tokenHeatmapLevel,
  weekdayLabels,
} from "./calendar";

describe("pad2", () => {
  it("left-pads single digits with a zero", () => {
    expect(pad2(5)).toBe("05");
    expect(pad2(12)).toBe("12");
  });
});

describe("toDateValue / parseDateValue", () => {
  it("round-trips a date through the yyyy-mm-dd format", () => {
    const date = new Date(2026, 7, 18);
    expect(toDateValue(date)).toBe("2026-08-18");
    const parsed = parseDateValue("2026-08-18");
    expect(parsed).not.toBeNull();
    expect(parsed?.getFullYear()).toBe(2026);
    expect(parsed?.getMonth()).toBe(7);
    expect(parsed?.getDate()).toBe(18);
  });

  it("rejects malformed or out-of-range dates", () => {
    expect(parseDateValue("not-a-date")).toBeNull();
    expect(parseDateValue("2026-13-40")).toBeNull();
    expect(parseDateValue("2026-02-30")).toBeNull();
  });
});

describe("formatDateLabel", () => {
  it("shows a placeholder for invalid input", () => {
    expect(formatDateLabel("nope")).toBe("选择日期");
  });

  it("formats a valid date value", () => {
    expect(formatDateLabel("2026-08-18")).toBe("2026-08-18");
  });
});

describe("monthTitle", () => {
  it("shows a 1-indexed month in Chinese", () => {
    expect(monthTitle(2026, 0)).toBe("2026 年 1 月");
    expect(monthTitle(2026, 11)).toBe("2026 年 12 月");
  });
});

describe("weekdayLabels", () => {
  it("returns the Monday-first Chinese weekday labels", () => {
    expect(weekdayLabels()).toEqual(["一", "二", "三", "四", "五", "六", "日"]);
  });
});

describe("calendarCells", () => {
  it("returns 42 cells with Monday as the first column", () => {
    const cells = calendarCells(2026, 7);
    expect(cells).toHaveLength(42);
    const first = parseDateValue(cells[0].value);
    expect(first?.getDay()).toBe(1);
  });

  it("marks cells outside the requested month as inMonth=false", () => {
    const cells = calendarCells(2026, 7);
    expect(cells.some((cell) => !cell.inMonth)).toBe(true);
    const inMonthCount = cells.filter((cell) => cell.inMonth).length;
    expect(inMonthCount).toBe(31);
  });
});

describe("shiftMonth", () => {
  it("moves forward across a year boundary", () => {
    expect(shiftMonth(2026, 11, 1)).toEqual({ year: 2027, month: 0 });
  });

  it("moves backward across a year boundary", () => {
    expect(shiftMonth(2026, 0, -1)).toEqual({ year: 2025, month: 11 });
  });
});

describe("heatmapWindow", () => {
  it("spans exactly 53 weeks (371 days) ending on the given date", () => {
    // 2026-08-16 is a Sunday, so it lands cleanly on a week boundary.
    const end = new Date(2026, 7, 16);
    const window = heatmapWindow(end);
    expect(window.toDate).toBe("2026-08-16");
    const from = parseDateValue(window.fromDate);
    const to = parseDateValue(window.toDate);
    expect(from).not.toBeNull();
    expect(to).not.toBeNull();
    const spanDays = Math.round((to!.getTime() - from!.getTime()) / (24 * 3600 * 1000)) + 1;
    expect(spanDays).toBe(371);
    expect(from?.getDay()).toBe(1);
  });

  it("falls back to now for an invalid end date", () => {
    const window = heatmapWindow(new Date(NaN));
    expect(window.fromDate).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(window.toDate).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});

describe("heatmapGrid", () => {
  it("returns an empty grid for invalid or inverted ranges", () => {
    expect(heatmapGrid("nope", "2026-08-01")).toEqual([]);
    expect(heatmapGrid("2026-08-10", "2026-08-01")).toEqual([]);
  });

  it("builds full weeks of 7 days each, marking days after `to` as future", () => {
    const weeks = heatmapGrid("2026-08-03", "2026-08-05");
    expect(weeks).toHaveLength(1);
    expect(weeks[0].days).toHaveLength(7);
    expect(weeks[0].days.map((day) => day.future)).toEqual([
      false,
      false,
      false,
      true,
      true,
      true,
      true,
    ]);
  });
});

describe("heatmapMonthLabels", () => {
  it("labels the first visible week even if it doesn't contain the 1st", () => {
    const weeks = heatmapGrid("2026-08-03", "2026-08-10");
    const labels = heatmapMonthLabels(weeks);
    expect(labels[0]).toEqual({ label: "8月", weekIndex: 0 });
  });

  it("skips a new label when it would land within 2 weeks of the previous one", () => {
    const weeks = heatmapGrid("2026-07-27", "2026-08-10");
    const labels = heatmapMonthLabels(weeks);
    const weekIndices = labels.map((label) => label.weekIndex);
    expect(new Set(weekIndices).size).toBe(weekIndices.length);
  });
});

describe("quantileCuts", () => {
  it("returns an empty array for no values", () => {
    expect(quantileCuts([])).toEqual([]);
  });

  it("returns deduplicated, sorted quantile boundaries", () => {
    const cuts = quantileCuts([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    expect(cuts[cuts.length - 1]).toBe(10);
    expect(cuts).toEqual([...cuts].sort((a, b) => a - b));
    expect(new Set(cuts).size).toBe(cuts.length);
  });
});

describe("bucketToDateRange", () => {
  it("maps hour and day buckets to that calendar day", () => {
    expect(bucketToDateRange("hour", "2026-08-01T11")).toEqual({
      from: "2026-08-01",
      to: "2026-08-01",
    });
    expect(bucketToDateRange("day", "2026-08-01")).toEqual({
      from: "2026-08-01",
      to: "2026-08-01",
    });
  });

  it("maps ISO week buckets to Monday–Sunday", () => {
    expect(bucketToDateRange("week", "2026-W31")).toEqual({
      from: "2026-07-27",
      to: "2026-08-02",
    });
    expect(bucketToDateRange("week", "2026-W32")).toEqual({
      from: "2026-08-03",
      to: "2026-08-09",
    });
    expect(bucketToDateRange("week", "2026-W01")).toEqual({
      from: "2025-12-29",
      to: "2026-01-04",
    });
  });

  it("maps month buckets to the first and last calendar day", () => {
    expect(bucketToDateRange("month", "2026-08")).toEqual({
      from: "2026-08-01",
      to: "2026-08-31",
    });
    expect(bucketToDateRange("month", "2026-02")).toEqual({
      from: "2026-02-01",
      to: "2026-02-28",
    });
    expect(bucketToDateRange("month", "2024-02")).toEqual({
      from: "2024-02-01",
      to: "2024-02-29",
    });
  });

  it("rejects malformed or out-of-range buckets", () => {
    expect(bucketToDateRange("hour", "2026-08-01")).toBeNull();
    expect(bucketToDateRange("hour", "2026-08-01T24")).toBeNull();
    expect(bucketToDateRange("day", "2026-02-30")).toBeNull();
    expect(bucketToDateRange("week", "2026-W00")).toBeNull();
    expect(bucketToDateRange("week", "2026-31")).toBeNull();
    expect(bucketToDateRange("month", "2026-13")).toBeNull();
  });
});

describe("tokenHeatmapLevel", () => {
  it("returns 0 for non-positive values", () => {
    expect(tokenHeatmapLevel(0, [10, 20, 30])).toBe(0);
    expect(tokenHeatmapLevel(-5, [10, 20, 30])).toBe(0);
  });

  it("maps a value into the bucket it falls under", () => {
    const cuts = [10, 20, 30];
    expect(tokenHeatmapLevel(5, cuts)).toBe(1);
    expect(tokenHeatmapLevel(15, cuts)).toBe(2);
    expect(tokenHeatmapLevel(25, cuts)).toBe(3);
  });

  it("caps the level at 4 even with more than 4 distinct cuts", () => {
    const cuts = [10, 20, 30, 40, 50, 60];
    expect(tokenHeatmapLevel(1000, cuts)).toBe(4);
  });
});

describe("placeHeatmapTooltip", () => {
  const tip = { width: 180, height: 40 };
  const viewport = { width: 1000, height: 800 };

  it("centers above a middle cell", () => {
    expect(
      placeHeatmapTooltip({ left: 500, top: 200, right: 512, bottom: 212 }, tip, viewport),
    ).toEqual({ left: 416, top: 152 });
  });

  it("shifts left so a right-edge cell stays inside the viewport", () => {
    const placed = placeHeatmapTooltip(
      { left: 980, top: 200, right: 992, bottom: 212 },
      tip,
      viewport,
    );
    expect(placed.left).toBe(812);
    expect(placed.left + tip.width).toBeLessThanOrEqual(viewport.width - 8);
  });

  it("shifts right so a left-edge cell stays inside the viewport", () => {
    expect(
      placeHeatmapTooltip({ left: 4, top: 200, right: 16, bottom: 212 }, tip, viewport),
    ).toEqual({ left: 8, top: 152 });
  });

  it("opens below when there is no room above", () => {
    expect(
      placeHeatmapTooltip({ left: 500, top: 20, right: 512, bottom: 32 }, tip, viewport),
    ).toEqual({ left: 416, top: 40 });
  });
});
