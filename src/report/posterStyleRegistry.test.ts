import { describe, expect, it } from "vitest";
import { DarkAnalyticsPoster } from "./darkAnalyticsPoster";
import {
  DEFAULT_REPORT_POSTER_STYLE_ID,
  REPORT_POSTER_STYLES,
  isReportPosterStyleId,
  resolveReportPosterStyle,
  resolveReportPosterStyleId,
} from "./posterStyleRegistry";

const HEX_COLOR = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
const STYLE_ID = /^[a-z][a-z0-9]*(-[a-z0-9]+)*$/;

describe("report poster style registry", () => {
  it("registers the current poster as the only default style", () => {
    const ids = REPORT_POSTER_STYLES.map((style) => style.id);
    expect(ids).toEqual(["dark-analytics"]);
    expect(DEFAULT_REPORT_POSTER_STYLE_ID).toBe("dark-analytics");
    expect(ids).toContain(DEFAULT_REPORT_POSTER_STYLE_ID);
    expect(resolveReportPosterStyle(DEFAULT_REPORT_POSTER_STYLE_ID).Component).toBe(
      DarkAnalyticsPoster,
    );
  });

  it("keeps ids unique, labels non-empty, swatches valid, and components renderable", () => {
    const ids = REPORT_POSTER_STYLES.map((style) => style.id);
    expect(new Set(ids).size).toBe(ids.length);

    for (const style of REPORT_POSTER_STYLES) {
      expect(style.id).toMatch(STYLE_ID);
      expect(style.label.trim().length).toBeGreaterThan(0);
      expect(style.swatch.background).toMatch(HEX_COLOR);
      expect(style.swatch.accent).toMatch(HEX_COLOR);
      expect(typeof style.Component).toBe("function");
    }
  });

  it("falls back to dark-analytics for missing, empty, or unknown ids", () => {
    expect(resolveReportPosterStyleId(undefined)).toBe("dark-analytics");
    expect(resolveReportPosterStyleId(null)).toBe("dark-analytics");
    expect(resolveReportPosterStyleId("")).toBe("dark-analytics");
    expect(resolveReportPosterStyleId("legacy")).toBe("dark-analytics");
    expect(resolveReportPosterStyleId("light-glass")).toBe("dark-analytics");
    expect(isReportPosterStyleId("dark-analytics")).toBe(true);
    expect(isReportPosterStyleId("legacy")).toBe(false);
    expect(resolveReportPosterStyle("not-a-style").id).toBe("dark-analytics");
    expect(resolveReportPosterStyle("not-a-style").Component).toBe(DarkAnalyticsPoster);
  });
});
