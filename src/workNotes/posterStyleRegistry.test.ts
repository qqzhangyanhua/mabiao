import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { REPORT_POSTER_STYLES } from "../report/posterStyleRegistry";
import { CinnabarSlipPoster } from "./cinnabarSlipPoster";
import { DuskBriefPoster } from "./duskBriefPoster";
import { FolioRuledPoster } from "./folioRuledPoster";
import {
  DEFAULT_WORK_NOTES_POSTER_STYLE_ID,
  WORK_NOTES_POSTER_STYLES,
  isWorkNotesPosterStyleId,
  resolveWorkNotesPosterStyle,
  resolveWorkNotesPosterStyleId,
} from "./posterStyleRegistry";

const HEX_COLOR = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
const STYLE_ID = /^[a-z][a-z0-9]*(-[a-z0-9]+)*$/;
const REPORT_STYLE_IDS = [
  "dark-analytics",
  "light-glass",
  "bauhaus-print",
  "newsprint",
  "ink-wash",
  "ticket-stub",
  "fuse-bead",
  "cast-concrete",
] as const;

describe("work notes poster style registry", () => {
  it("registers three DOM styles and keeps them off the report registry", () => {
    const ids = WORK_NOTES_POSTER_STYLES.map((style) => style.id);
    expect(ids).toEqual(["folio-ruled", "dusk-brief", "cinnabar-slip"]);
    expect(DEFAULT_WORK_NOTES_POSTER_STYLE_ID).toBe("folio-ruled");
    expect(new Set(ids).size).toBe(ids.length);
    expect(REPORT_POSTER_STYLES.map((style) => style.id)).toEqual([...REPORT_STYLE_IDS]);
    for (const id of ids) {
      expect(REPORT_STYLE_IDS, id).not.toContain(id);
    }
  });

  it("keeps ids unique, labels Chinese, swatches valid, and stylesheets local", () => {
    for (const style of WORK_NOTES_POSTER_STYLES) {
      expect(style.id).toMatch(STYLE_ID);
      expect(style.label.trim().length).toBeGreaterThan(0);
      expect(style.label).toMatch(/[\u4e00-\u9fff]/);
      expect(style.stylesheet).toMatch(/^[a-z][a-zA-Z0-9]*Poster\.css$/);
      expect(style.swatch.background).toMatch(HEX_COLOR);
      expect(style.swatch.accent).toMatch(HEX_COLOR);
      expect(typeof style.Component).toBe("function");
    }
  });

  it("resolves each style to its DOM component", () => {
    expect(resolveWorkNotesPosterStyle("folio-ruled")).toMatchObject({
      id: "folio-ruled",
      label: "栏线手札",
      stylesheet: "folioRuledPoster.css",
      Component: FolioRuledPoster,
    });
    expect(resolveWorkNotesPosterStyle("dusk-brief")).toMatchObject({
      id: "dusk-brief",
      label: "暮色简报",
      stylesheet: "duskBriefPoster.css",
      Component: DuskBriefPoster,
    });
    expect(resolveWorkNotesPosterStyle("cinnabar-slip")).toMatchObject({
      id: "cinnabar-slip",
      label: "朱砂条",
      stylesheet: "cinnabarSlipPoster.css",
      Component: CinnabarSlipPoster,
    });
  });

  it("falls back to folio-ruled for missing, empty, report, or unknown ids", () => {
    expect(resolveWorkNotesPosterStyleId(undefined)).toBe("folio-ruled");
    expect(resolveWorkNotesPosterStyleId(null)).toBe("folio-ruled");
    expect(resolveWorkNotesPosterStyleId("")).toBe("folio-ruled");
    expect(resolveWorkNotesPosterStyleId("not-a-style")).toBe("folio-ruled");
    expect(resolveWorkNotesPosterStyleId("dark-analytics")).toBe("folio-ruled");
    expect(resolveWorkNotesPosterStyleId("Folio-Ruled")).toBe("folio-ruled");
    expect(resolveWorkNotesPosterStyleId(42)).toBe("folio-ruled");
    expect(isWorkNotesPosterStyleId("folio-ruled")).toBe(true);
    expect(isWorkNotesPosterStyleId("dark-analytics")).toBe(false);
    expect(resolveWorkNotesPosterStyle("dark-analytics").Component).toBe(FolioRuledPoster);
  });

  it("does not introduce canvas paint or screenshot libraries", () => {
    const files = [
      "folioRuledPoster.tsx",
      "duskBriefPoster.tsx",
      "cinnabarSlipPoster.tsx",
      "WorkNotesPoster.tsx",
    ];
    for (const file of files) {
      const source = readFileSync(new URL(file, import.meta.url), "utf8");
      expect(source, file).not.toMatch(/data-poster-canvas|<canvas\b|html2canvas|echarts/i);
    }
  });
});
