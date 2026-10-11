import { describe, expect, it } from "vitest";
import { formatChars, formatCost, formatDateTime, formatPercent, formatRelativeDay, formatTokens } from "./format";

describe("formatTokens", () => {
  it.each([
    [0, "0"],
    [999, "999"],
    [1500, "1.5K"],
    [2_500_000, "2.50M"],
    [3_000_000_000, "3.00B"],
  ])("%d → %s", (input, expected) => {
    expect(formatTokens(input)).toBe(expected);
  });
});

describe("formatCost", () => {
  it.each([
    [0, "$0"],
    [0.01234, "$0.0123"],
    [12.345, "$12.35"],
  ])("%d → %s", (input, expected) => {
    expect(formatCost(input)).toBe(expected);
  });
});

describe("formatChars", () => {
  it("uses Chinese grouping and does not reuse the token K/M suffix", () => {
    expect(formatChars(1234)).toBe("1,234 字符");
    expect(formatChars(0)).toBe("0 字符");
  });
});

describe("formatPercent", () => {
  it("keeps one decimal below 10%", () => {
    expect(formatPercent(0.05)).toBe("5.0%");
    expect(formatPercent(0.25)).toBe("25%");
  });
});

describe("formatRelativeDay", () => {
  const now = new Date(2026, 2, 11, 15, 0, 0);

  it("shows a dash for missing times", () => {
    expect(formatRelativeDay(null, now)).toBe("—");
  });

  it("speaks in today / yesterday / N days ago", () => {
    expect(formatRelativeDay(new Date(2026, 2, 11, 8).toISOString(), now)).toBe("今天");
    expect(formatRelativeDay(new Date(2026, 2, 10, 8).toISOString(), now)).toBe("昨天");
    expect(formatRelativeDay(new Date(2026, 2, 8, 8).toISOString(), now)).toBe("3 天前");
  });
});

describe("formatDateTime", () => {
  it("shows a dash for missing or invalid times", () => {
    expect(formatDateTime(null)).toBe("—");
    expect(formatDateTime("not a time")).toBe("—");
  });

  it("formats a valid time as local date and minute", () => {
    expect(formatDateTime("2026-03-01T10:05:00Z")).toMatch(/^2026-0[23]-\d{2} \d{2}:05$/);
  });
});
