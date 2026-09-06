import { readdirSync, readFileSync } from "node:fs";
import {
  extractCssImports,
  findExportedComponentFile,
  parseCssCustomPropertyNames,
} from "./posterCssGuard";
import {
  WORK_NOTES_POSTER_STYLES,
  type WorkNotesPosterStyle,
} from "../workNotes/posterStyleRegistry";
import { REPORT_POSTER_STYLES, type ReportPosterStyle } from "./posterStyleRegistry";

export const SPIKE_HARNESS_STYLESHEET = "spike.css";

const REPORT_DIR = new URL("./", import.meta.url);
const WORK_NOTES_DIR = new URL("../workNotes/", import.meta.url);
const TOKENS_URL = new URL("../styles/base/tokens.css", import.meta.url);

export type CoveredPosterCss = {
  name: string;
  kind: "weekly-report" | "work-notes" | "harness";
  source: string;
};

type PosterStyleEntry = ReportPosterStyle | WorkNotesPosterStyle;

type SourceFile = {
  path: string;
  source: string;
  root: URL;
};

export type PosterCssCoverageError = {
  styleId: string;
  reason: string;
};

function listRelativeFiles(root: URL, predicate: (name: string) => boolean): string[] {
  const names = readdirSync(root, { recursive: true });
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

function listSourceFiles(root: URL): SourceFile[] {
  return listRelativeFiles(
    root,
    (name) =>
      (name.endsWith(".ts") || name.endsWith(".tsx")) &&
      !name.endsWith(".test.ts") &&
      !name.endsWith(".test.tsx"),
  ).map((relative) => ({
    path: relative,
    source: readFileSync(new URL(relative, root), "utf8"),
    root,
  }));
}

function fileExists(url: URL): boolean {
  try {
    readFileSync(url, "utf8");
    return true;
  } catch {
    return false;
  }
}

function relativeToRoot(url: URL, root: URL): string {
  const base = root.href;
  if (url.href.startsWith(base)) {
    return decodeURIComponent(url.href.slice(base.length));
  }
  return url.pathname;
}

function coverStyle(
  style: PosterStyleEntry,
  kind: CoveredPosterCss["kind"],
  root: URL,
  modules: readonly SourceFile[],
  covered: Map<string, CoveredPosterCss>,
  errors: PosterCssCoverageError[],
): void {
  const stylesheetUrl = new URL(style.stylesheet, root);
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
  const owner = modules.find((file) => file.path === componentFile);
  if (!componentFile || !owner) {
    errors.push({
      styleId: style.id,
      reason: `cannot locate a unique export for ${componentName || "(anonymous component)"}`,
    });
    return;
  }

  const imported = extractCssImports(owner.source);
  if (imported.length === 0) {
    errors.push({
      styleId: style.id,
      reason: `${componentFile} imports no CSS; a registered style cannot silently skip the guard`,
    });
    return;
  }

  const importedUrls = imported.map((spec) => new URL(spec, new URL(componentFile, owner.root)));
  if (!importedUrls.some((url) => url.href === stylesheetUrl.href)) {
    errors.push({
      styleId: style.id,
      reason: `${componentFile} does not import registered stylesheet ${style.stylesheet}`,
    });
  }

  covered.set(stylesheetUrl.href, {
    name: relativeToRoot(stylesheetUrl, root),
    kind,
    source: stylesheetSource,
  });

  for (const cssUrl of importedUrls) {
    if (!fileExists(cssUrl)) {
      errors.push({
        styleId: style.id,
        reason: `imported stylesheet ${relativeToRoot(cssUrl, root)} is missing`,
      });
      continue;
    }
    const source = readFileSync(cssUrl, "utf8");
    if (source.trim().length === 0) {
      errors.push({
        styleId: style.id,
        reason: `imported stylesheet ${relativeToRoot(cssUrl, root)} is empty`,
      });
      continue;
    }
    if (!covered.has(cssUrl.href)) {
      covered.set(cssUrl.href, {
        name: relativeToRoot(cssUrl, root),
        kind,
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

function coverExtraCss(
  root: URL,
  kind: CoveredPosterCss["kind"],
  covered: Map<string, CoveredPosterCss>,
): void {
  for (const relative of listRelativeFiles(root, (name) => name.endsWith(".css"))) {
    const url = new URL(relative, root);
    if (covered.has(url.href)) {
      continue;
    }
    covered.set(url.href, {
      name: relative,
      kind,
      source: readFileSync(url, "utf8"),
    });
  }
}

/**
 * 从周报与纪要注册表推海报 CSS，并并入 spike 夹具。
 * 目录里多出来的海报 CSS 也会进隔离门禁，避免漏网。
 */
export function collectPosterCssCoverage(): {
  files: CoveredPosterCss[];
  errors: PosterCssCoverageError[];
  appTokenNames: string[];
} {
  const modules = [...listSourceFiles(REPORT_DIR), ...listSourceFiles(WORK_NOTES_DIR)];

  const covered = new Map<string, CoveredPosterCss>();
  const errors: PosterCssCoverageError[] = [];

  for (const style of REPORT_POSTER_STYLES) {
    coverStyle(style, "weekly-report", REPORT_DIR, modules, covered, errors);
  }
  for (const style of WORK_NOTES_POSTER_STYLES) {
    coverStyle(style, "work-notes", WORK_NOTES_DIR, modules, covered, errors);
  }

  coverNamedFile(SPIKE_HARNESS_STYLESHEET, "harness", covered, errors);

  coverExtraCss(REPORT_DIR, "weekly-report", covered);
  coverExtraCss(WORK_NOTES_DIR, "work-notes", covered);

  return {
    files: [...covered.values()].sort((left, right) => left.name.localeCompare(right.name)),
    errors,
    appTokenNames: parseCssCustomPropertyNames(readFileSync(TOKENS_URL, "utf8")),
  };
}
