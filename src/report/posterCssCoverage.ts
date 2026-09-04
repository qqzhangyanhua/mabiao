import { readdirSync, readFileSync } from "node:fs";
import {
  extractCssImports,
  findExportedComponentFile,
  parseCssCustomPropertyNames,
} from "./posterCssGuard";
import { REPORT_POSTER_STYLES, type ReportPosterStyle } from "./posterStyleRegistry";

export const SPIKE_HARNESS_STYLESHEET = "spike.css";

const REPORT_DIR = new URL("./", import.meta.url);
const TOKENS_URL = new URL("../styles/base/tokens.css", import.meta.url);

export type CoveredPosterCss = {
  name: string;
  kind: "weekly-report" | "harness";
  source: string;
};

export type PosterCssCoverageError = {
  styleId: string;
  reason: string;
};

function listRelativeFiles(predicate: (name: string) => boolean): string[] {
  const names = readdirSync(REPORT_DIR, { recursive: true });
  const files: string[] = [];
  for (const name of names) {
    if (typeof name !== "string") {
      continue;
    }
    const relative = name.replaceAll("\\", "/");
    if (predicate(relative)) {
      files.push(relative);
    }
  }
  return files;
}

function readReportFile(relative: string): string {
  return readFileSync(new URL(relative, REPORT_DIR), "utf8");
}

function fileExists(url: URL): boolean {
  try {
    readFileSync(url, "utf8");
    return true;
  } catch {
    return false;
  }
}

function relativeToReport(url: URL): string {
  const base = REPORT_DIR.href;
  if (url.href.startsWith(base)) {
    return decodeURIComponent(url.href.slice(base.length));
  }
  return url.pathname;
}

function coverStyle(
  style: ReportPosterStyle,
  modules: readonly { path: string; source: string }[],
  covered: Map<string, CoveredPosterCss>,
  errors: PosterCssCoverageError[],
): void {
  const stylesheetUrl = new URL(style.stylesheet, REPORT_DIR);
  if (!fileExists(stylesheetUrl)) {
    errors.push({
      styleId: style.id,
      reason: `registered stylesheet ${style.stylesheet} is missing`,
    });
    return;
  }
  const stylesheetSource = readFileSync(stylesheetUrl, "utf8");
  if (stylesheetSource.trim().length === 0) {
    errors.push({
      styleId: style.id,
      reason: `registered stylesheet ${style.stylesheet} is empty`,
    });
    return;
  }

  const componentName = style.Component.name;
  const componentFile = findExportedComponentFile(modules, componentName);
  if (!componentFile) {
    errors.push({
      styleId: style.id,
      reason: `cannot locate a unique export for ${componentName || "(anonymous component)"}`,
    });
    return;
  }

  const imported = extractCssImports(modules.find((file) => file.path === componentFile)?.source ?? "");
  if (imported.length === 0) {
    errors.push({
      styleId: style.id,
      reason: `${componentFile} imports no CSS; a registered style cannot silently skip the guard`,
    });
    return;
  }

  const importedUrls = imported.map((spec) => new URL(spec, new URL(componentFile, REPORT_DIR)));
  if (!importedUrls.some((url) => url.href === stylesheetUrl.href)) {
    errors.push({
      styleId: style.id,
      reason: `${componentFile} does not import registered stylesheet ${style.stylesheet}`,
    });
  }

  covered.set(stylesheetUrl.href, {
    name: relativeToReport(stylesheetUrl),
    kind: "weekly-report",
    source: stylesheetSource,
  });

  for (const cssUrl of importedUrls) {
    if (!fileExists(cssUrl)) {
      errors.push({
        styleId: style.id,
        reason: `imported stylesheet ${relativeToReport(cssUrl)} is missing`,
      });
      continue;
    }
    const source = readFileSync(cssUrl, "utf8");
    if (source.trim().length === 0) {
      errors.push({
        styleId: style.id,
        reason: `imported stylesheet ${relativeToReport(cssUrl)} is empty`,
      });
      continue;
    }
    if (!covered.has(cssUrl.href)) {
      covered.set(cssUrl.href, {
        name: relativeToReport(cssUrl),
        kind: "weekly-report",
        source,
      });
    }
  }
}

function coverNamedFile(
  filename: string,
  kind: CoveredPosterCss["kind"],
  covered: Map<string, CoveredPosterCss>,
  errors: PosterCssCoverageError[],
): void {
  const url = new URL(filename, REPORT_DIR);
  if (!fileExists(url)) {
    errors.push({ styleId: filename, reason: `${filename} is missing` });
    return;
  }
  const source = readFileSync(url, "utf8");
  if (source.trim().length === 0) {
    errors.push({ styleId: filename, reason: `${filename} is empty` });
    return;
  }
  covered.set(url.href, { name: filename, kind, source });
}

/**
 * 从注册表推每周报 CSS，并并入 spike 夹具。
 * 目录里多出来的海报 CSS 也会进隔离门禁，避免漏网。
 */
export function collectPosterCssCoverage(): {
  files: CoveredPosterCss[];
  errors: PosterCssCoverageError[];
  appTokenNames: string[];
} {
  const modules = listRelativeFiles(
    (name) =>
      (name.endsWith(".ts") || name.endsWith(".tsx")) &&
      !name.endsWith(".test.ts") &&
      !name.endsWith(".test.tsx"),
  ).map((relative) => ({ path: relative, source: readReportFile(relative) }));

  const covered = new Map<string, CoveredPosterCss>();
  const errors: PosterCssCoverageError[] = [];

  for (const style of REPORT_POSTER_STYLES) {
    coverStyle(style, modules, covered, errors);
  }

  coverNamedFile(SPIKE_HARNESS_STYLESHEET, "harness", covered, errors);

  for (const relative of listRelativeFiles((name) => name.endsWith(".css"))) {
    const url = new URL(relative, REPORT_DIR);
    if (covered.has(url.href)) {
      continue;
    }
    covered.set(url.href, {
      name: relative,
      kind: "weekly-report",
      source: readReportFile(relative),
    });
  }

  return {
    files: [...covered.values()].sort((left, right) => left.name.localeCompare(right.name)),
    errors,
    appTokenNames: parseCssCustomPropertyNames(readFileSync(TOKENS_URL, "utf8")),
  };
}
