import { describe, expect, it } from "vitest";
import { BauhausPoster } from "./bauhausPoster";
import { CastConcretePoster } from "./castConcretePoster";
import { DarkAnalyticsPoster } from "./darkAnalyticsPoster";
import { FuseBeadPoster } from "./fuseBeadPoster";
import { InkWashPoster } from "./inkWashPoster";
import { LightGlassPoster } from "./lightGlassPoster";
import { NewsprintPoster } from "./newsprintPoster";
import { TicketStubPoster } from "./ticketStubPoster";
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

  it("registers bauhaus-print with a Chinese label, swatch, stylesheet, and BauhausPoster", () => {
    const style = resolveReportPosterStyle("bauhaus-print");
    expect(style.id).toBe("bauhaus-print");
    expect(style.label).toBe("构成海报");
    expect(style.stylesheet).toBe("bauhausPoster.css");
    expect(style.swatch.background).toBe("#f6f1e6");
    expect(style.swatch.accent).toBe("#e30613");
    expect(style.Component).toBe(BauhausPoster);
    expect(isReportPosterStyleId("bauhaus-print")).toBe(true);
    expect(resolveReportPosterStyleId("bauhaus-print")).toBe("bauhaus-print");
  });

  it("registers newsprint with a Chinese label, swatch, stylesheet, and NewsprintPoster", () => {
    const style = resolveReportPosterStyle("newsprint");
    expect(style.id).toBe("newsprint");
    expect(style.label).toBe("旧报号外");
    expect(style.stylesheet).toBe("newsprintPoster.css");
    expect(style.swatch.background).toBe("#e7d6b4");
    expect(style.swatch.accent).toBe("#1c1610");
    expect(style.Component).toBe(NewsprintPoster);
    expect(isReportPosterStyleId("newsprint")).toBe(true);
    expect(resolveReportPosterStyleId("newsprint")).toBe("newsprint");
  });

  it("registers ink-wash with a Chinese label, swatch, stylesheet, and InkWashPoster", () => {
    const style = resolveReportPosterStyle("ink-wash");
    expect(style.id).toBe("ink-wash");
    expect(style.label).toBe("水墨手札");
    expect(style.stylesheet).toBe("inkWashPoster.css");
    expect(style.swatch.background).toBe("#f4efe6");
    expect(style.swatch.accent).toBe("#9c3b32");
    expect(style.Component).toBe(InkWashPoster);
    expect(isReportPosterStyleId("ink-wash")).toBe(true);
    expect(resolveReportPosterStyleId("ink-wash")).toBe("ink-wash");
  });

  it("registers ticket-stub with a Chinese label, swatch, stylesheet, and TicketStubPoster", () => {
    const style = resolveReportPosterStyle("ticket-stub");
    expect(style.id).toBe("ticket-stub");
    expect(style.label).toBe("票据存根");
    expect(style.stylesheet).toBe("ticketStubPoster.css");
    expect(style.swatch.background).toBe("#f3ead8");
    expect(style.swatch.accent).toBe("#c45c4a");
    expect(style.Component).toBe(TicketStubPoster);
    expect(isReportPosterStyleId("ticket-stub")).toBe(true);
    expect(resolveReportPosterStyleId("ticket-stub")).toBe("ticket-stub");
  });

  it("registers fuse-bead with a Chinese label, swatch, stylesheet, and FuseBeadPoster", () => {
    const style = resolveReportPosterStyle("fuse-bead");
    expect(style.id).toBe("fuse-bead");
    expect(style.label).toBe("拼豆海报");
    expect(style.stylesheet).toBe("fuseBeadPoster.css");
    expect(style.swatch.background).toBe("#eef0f5");
    expect(style.swatch.accent).toBe("#8b5cf6");
    expect(style.Component).toBe(FuseBeadPoster);
    expect(isReportPosterStyleId("fuse-bead")).toBe(true);
    expect(resolveReportPosterStyleId("fuse-bead")).toBe("fuse-bead");
  });

  it("registers cast-concrete with a Chinese label, swatch, stylesheet, and CastConcretePoster", () => {
    const style = resolveReportPosterStyle("cast-concrete");
    expect(style.id).toBe("cast-concrete");
    expect(style.label).toBe("清水混凝土");
    expect(style.stylesheet).toBe("castConcretePoster.css");
    expect(style.swatch.background).toBe("#b6b5af");
    expect(style.swatch.accent).toBe("#7a7872");
    expect(style.Component).toBe(CastConcretePoster);
    expect(isReportPosterStyleId("cast-concrete")).toBe(true);
    expect(resolveReportPosterStyleId("cast-concrete")).toBe("cast-concrete");
  });

  it("falls back to dark-analytics for missing, empty, or unknown ids", () => {
    expect(resolveReportPosterStyleId(undefined)).toBe("dark-analytics");
    expect(resolveReportPosterStyleId(null)).toBe("dark-analytics");
    expect(resolveReportPosterStyleId("")).toBe("dark-analytics");
    expect(resolveReportPosterStyleId("not-a-style")).toBe("dark-analytics");
    expect(resolveReportPosterStyleId("purple-glass")).toBe("dark-analytics");
    expect(resolveReportPosterStyleId("cyber-neon")).toBe("dark-analytics");
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
