import { describe, expect, it } from "vitest";
import { presetRange, queryBounds } from "./range";

const EAST_8 = 8 * 60;

describe("presetRange", () => {
  // UTC 2026-03-10 20:00 在东八区已经是 3 月 11 日 04:00。
  const now = new Date("2026-03-10T20:00:00Z");

  it("uses the calendar day of the requested offset, not UTC", () => {
    expect(presetRange("7d", now, EAST_8)).toEqual({ fromDay: "2026-03-05", toDay: "2026-03-11" });
    expect(presetRange("7d", now, 0)).toEqual({ fromDay: "2026-03-04", toDay: "2026-03-10" });
  });

  it("covers N days including today", () => {
    expect(presetRange("30d", now, 0)).toEqual({ fromDay: "2026-02-09", toDay: "2026-03-10" });
  });

  it("month starts on the 1st", () => {
    expect(presetRange("month", now, EAST_8)).toEqual({
      fromDay: "2026-03-01",
      toDay: "2026-03-11",
    });
  });
});

describe("queryBounds", () => {
  it("turns local days into UTC instants, end exclusive", () => {
    expect(queryBounds({ fromDay: "2026-03-01", toDay: "2026-03-02" }, EAST_8)).toEqual({
      from: "2026-02-28T16:00:00.000Z",
      to: "2026-03-02T16:00:00.000Z",
    });
  });

  it("a single day spans 24 hours", () => {
    expect(queryBounds({ fromDay: "2026-03-01", toDay: "2026-03-01" }, 0)).toEqual({
      from: "2026-03-01T00:00:00.000Z",
      to: "2026-03-02T00:00:00.000Z",
    });
  });

  it("rejects malformed or reversed ranges", () => {
    expect(queryBounds({ fromDay: "", toDay: "2026-03-01" }, 0)).toBeNull();
    expect(queryBounds({ fromDay: "03/01/2026", toDay: "2026-03-01" }, 0)).toBeNull();
    expect(queryBounds({ fromDay: "2026-03-05", toDay: "2026-03-01" }, 0)).toBeNull();
  });
});
