import { describe, expect, it } from "vitest";
import { collectPosterCssCoverage } from "./posterCssCoverage";
import {
  extractCssImports,
  findExportedComponentFile,
  inspectPosterCss,
  parseCssCustomPropertyNames,
  stripCssComments,
} from "./posterCssGuard";
import { REPORT_POSTER_STYLES } from "./posterStyleRegistry";

const APP_TOKENS = parseCssCustomPropertyNames(`
:root {
  --bg: #070b16;
  --purple: #8b6cff;
  --muted: #8b97ab;
}
`);

describe("inspectPosterCss", () => {
  it("allows color-mix and backdrop-filter on weekly report styles", () => {
    const css = `.glass {
  background: color-mix(in srgb, #ffffff 18%, transparent);
  backdrop-filter: blur(18px);
  -webkit-backdrop-filter: blur(18px);
}`;
    expect(inspectPosterCss(css, "weekly-report", APP_TOKENS)).toEqual([]);
  });

  it("rejects stylesheet @import on weekly report styles even when glass effects are present", () => {
    const css = `@import "../styles.css";
.glass { background: color-mix(in srgb, #fff 20%, transparent); }`;
    const issues = inspectPosterCss(css, "weekly-report", APP_TOKENS);
    expect(issues.map((issue) => issue.kind)).toEqual(["stylesheet_import", "main_stylesheet_reuse"]);
    expect(issues.some((issue) => issue.kind === "color_mix")).toBe(false);
  });

  it("rejects a hypothetical new style that points at the app stylesheet tree without @import", () => {
    const css = `.glass { background-image: url("./styles/base/tokens.css"); }`;
    expect(inspectPosterCss(css, "weekly-report", APP_TOKENS).map((issue) => issue.kind)).toEqual([
      "main_stylesheet_reuse",
    ]);
  });

  it("rejects a hypothetical new style that reuses app tokens", () => {
    const css = `.neon { color: var(--purple); background: var(--bg); }`;
    expect(inspectPosterCss(css, "weekly-report", APP_TOKENS).map((issue) => issue.kind)).toEqual([
      "main_stylesheet_reuse",
    ]);
  });

  it("does not treat poster-local tokens as app-token reuse", () => {
    const css = `.report-poster { background: var(--rp-bg); color: var(--rp-fg); }`;
    expect(inspectPosterCss(css, "weekly-report", APP_TOKENS)).toEqual([]);
  });

  it("still forbids color-mix and backdrop-filter on quota card CSS", () => {
    const mix = inspectPosterCss(".quota-poster { background: color-mix(in srgb, #fff 20%, #000); }", "quota-card", APP_TOKENS);
    const blur = inspectPosterCss(".quota-poster { backdrop-filter: blur(12px); }", "quota-card", APP_TOKENS);
    const webkit = inspectPosterCss(".quota-poster { -webkit-backdrop-filter: blur(12px); }", "quota-card", APP_TOKENS);
    expect(mix.map((issue) => issue.kind)).toEqual(["color_mix"]);
    expect(blur.map((issue) => issue.kind)).toEqual(["backdrop_filter"]);
    expect(webkit.map((issue) => issue.kind)).toEqual(["backdrop_filter"]);
  });

  it("rejects quota card @import and main stylesheet reuse", () => {
    const imported = inspectPosterCss('@import "./tokens.css";', "quota-card", APP_TOKENS);
    const reused = inspectPosterCss(".quota-poster { color: var(--muted); }", "quota-card", APP_TOKENS);
    expect(imported.map((issue) => issue.kind)).toEqual(["stylesheet_import"]);
    expect(reused.map((issue) => issue.kind)).toEqual(["main_stylesheet_reuse"]);
  });

  it("ignores forbidden tokens that only appear inside comments", () => {
    const css = `/* @import "../styles.css"; color-mix(); backdrop-filter: blur(1px); */
.report-poster { color: #fff; }`;
    expect(stripCssComments(css)).not.toMatch(/@import/);
    expect(inspectPosterCss(css, "quota-card", APP_TOKENS)).toEqual([]);
  });
});

describe("poster CSS coverage", () => {
  it("extracts CSS imports and locates exported style components", () => {
    expect(extractCssImports('import "./poster.css";\nexport function DarkAnalyticsPoster() {}')).toEqual([
      "./poster.css",
    ]);
    expect(
      findExportedComponentFile(
        [{ path: "darkAnalyticsPoster.tsx", source: "export function DarkAnalyticsPoster() {}" }],
        "DarkAnalyticsPoster",
      ),
    ).toBe("darkAnalyticsPoster.tsx");
  });

  it("covers every registered weekly report style plus quota and spike CSS", () => {
    const coverage = collectPosterCssCoverage();
    expect(coverage.errors, JSON.stringify(coverage.errors)).toEqual([]);
    expect(coverage.appTokenNames).toContain("bg");
    expect(coverage.appTokenNames).toContain("purple");

    const names = coverage.files.map((file) => file.name);
    expect(names).toContain("quotaPoster.css");
    expect(names).toContain("spike.css");
    for (const style of REPORT_POSTER_STYLES) {
      expect(names, `missing CSS for ${style.id}`).toContain(style.stylesheet);
    }

    const weekly = coverage.files.filter((file) => file.kind === "weekly-report");
    expect(weekly.length).toBeGreaterThanOrEqual(REPORT_POSTER_STYLES.length);
  });

  it("enforces isolation on every covered file and the quota effect ban", () => {
    const coverage = collectPosterCssCoverage();
    expect(coverage.errors).toEqual([]);

    for (const file of coverage.files) {
      expect(file.source.length, file.name).toBeGreaterThan(0);
      const issues = inspectPosterCss(file.source, file.kind, coverage.appTokenNames);
      expect(issues, file.name).toEqual([]);
    }
  });
});
