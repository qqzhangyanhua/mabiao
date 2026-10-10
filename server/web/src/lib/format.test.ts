import { describe, expect, it } from "vitest";
import { formatCost, formatDateTime, formatTokens } from "./format";

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

describe("formatDateTime", () => {
  it("shows a dash for missing or invalid times", () => {
    expect(formatDateTime(null)).toBe("—");
    expect(formatDateTime("not a time")).toBe("—");
  });

  it("formats a valid time as local date and minute", () => {
    expect(formatDateTime("2026-03-01T10:05:00Z")).toMatch(/^2026-0[23]-\d{2} \d{2}:05$/);
  });
});
