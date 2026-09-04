import { describe, expect, it } from "vitest";
import { DarkAnalyticsPoster } from "./darkAnalyticsPoster";
import { LightGlassPoster } from "./lightGlassPoster";
import { PurpleGlassPoster } from "./purpleGlassPoster";
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
  it("includes the default style and resolves it to DarkAnalyticsPoster", () => {
    const ids = REPORT_POSTER_STYLES.map((style) => style.id);
    expect(DEFAULT_REPORT_POSTER_STYLE_ID).toBe("dark-analytics");
    expect(ids).toContain(DEFAULT_REPORT_POSTER_STYLE_ID);
    expect(resolveReportPosterStyle(DEFAULT_REPORT_POSTER_STYLE_ID).Component).toBe(
      DarkAnalyticsPoster,
    );
    expect(resolveReportPosterStyle(undefined).Component).toBe(DarkAnalyticsPoster);
  });

  it("keeps ids unique, labels non-empty, swatches valid, and components renderable", () => {
    const ids = REPORT_POSTER_STYLES.map((style) => style.id);
    expect(new Set(ids).size).toBe(ids.length);

    for (const style of REPORT_POSTER_STYLES) {
      expect(style.id).toMatch(STYLE_ID);
      expect(style.label.trim().length).toBeGreaterThan(0);
      expect(style.stylesheet).toMatch(/^[a-z0-9].*\.css$/);
      expect(style.swatch.background).toMatch(HEX_COLOR);
      expect(style.swatch.accent).toMatch(HEX_COLOR);
      expect(typeof style.Component).toBe("function");
    }
  });

  it("registers light-glass with a Chinese label, swatch, stylesheet, and LightGlassPoster", () => {
    const style = resolveReportPosterStyle("light-glass");
    expect(style.id).toBe("light-glass");
    expect(style.label).toBe("浅色磨砂");
    expect(style.stylesheet).toBe("lightGlassPoster.css");
    expect(style.swatch.background).toBe("#eef6f8");
    expect(style.swatch.accent).toBe("#3d9aa8");
    expect(style.Component).toBe(LightGlassPoster);
    expect(isReportPosterStyleId("light-glass")).toBe(true);
    expect(resolveReportPosterStyleId("light-glass")).toBe("light-glass");
  });

  it("registers purple-glass with a Chinese label, swatch, stylesheet, and PurpleGlassPoster", () => {
    const style = resolveReportPosterStyle("purple-glass");
    expect(style.id).toBe("purple-glass");
    expect(style.label).toBe("紫蓝玻璃");
    expect(style.stylesheet).toBe("purpleGlassPoster.css");
    expect(style.swatch.background).toBe("#1b1460");
    expect(style.swatch.accent).toBe("#c4b5fd");
    expect(style.Component).toBe(PurpleGlassPoster);
    expect(isReportPosterStyleId("purple-glass")).toBe(true);
    expect(resolveReportPosterStyleId("purple-glass")).toBe("purple-glass");
  });

  it("falls back to dark-analytics for missing, empty, or unknown ids", () => {
    expect(resolveReportPosterStyleId(undefined)).toBe("dark-analytics");
    expect(resolveReportPosterStyleId(null)).toBe("dark-analytics");
    expect(resolveReportPosterStyleId("")).toBe("dark-analytics");
    expect(resolveReportPosterStyleId("not-a-style")).toBe("dark-analytics");
    expect(resolveReportPosterStyleId("Dark-Analytics")).toBe("dark-analytics");
    expect(resolveReportPosterStyleId(42)).toBe("dark-analytics");
    expect(isReportPosterStyleId("dark-analytics")).toBe(true);
    expect(isReportPosterStyleId("not-a-style")).toBe(false);
    expect(isReportPosterStyleId("Dark-Analytics")).toBe(false);
    expect(isReportPosterStyleId(42)).toBe(false);
    expect(resolveReportPosterStyle("not-a-style").id).toBe("dark-analytics");
    expect(resolveReportPosterStyle("not-a-style").Component).toBe(DarkAnalyticsPoster);
  });
});
