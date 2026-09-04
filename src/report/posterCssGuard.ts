/** 海报 CSS 隔离规则：周报风格可做玻璃/霓虹，与主样式表仍隔离。 */

export type PosterCssIssueKind = "stylesheet_import" | "main_stylesheet_reuse";

export type PosterCssIssue = {
  kind: PosterCssIssueKind;
  detail: string;
};

const IMPORT_RE = /@import\b/i;
const MAIN_STYLESHEET_PATH_RE = /(?:^|[^a-zA-Z0-9_-])(?:\.\.?\/)*styles(?:\.css|\/)/i;

export function stripCssComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, (block) => block.replace(/[^\n]/g, " "));
}

export function parseCssCustomPropertyNames(source: string): string[] {
  const names = new Set<string>();
  const stripped = stripCssComments(source);
  for (const match of stripped.matchAll(/--([a-z][a-z0-9-]*)\s*:/gi)) {
    const name = match[1];
    if (name) {
      names.add(name);
    }
  }
  return [...names];
}

export function usesCssCustomProperty(source: string, property: string): boolean {
  const escaped = property.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`var\\(\\s*--${escaped}(?:\\s*,|\\s*\\))`, "i").test(source);
}

export function inspectPosterCss(
  source: string,
  appTokenNames: readonly string[],
): PosterCssIssue[] {
  const css = stripCssComments(source);
  const issues: PosterCssIssue[] = [];

  if (IMPORT_RE.test(css)) {
    issues.push({ kind: "stylesheet_import", detail: "poster CSS must not @import another stylesheet" });
  }

  if (MAIN_STYLESHEET_PATH_RE.test(css)) {
    issues.push({
      kind: "main_stylesheet_reuse",
      detail: "poster CSS must not reference the application stylesheet tree",
    });
  } else {
    const reused = appTokenNames.filter((name) => usesCssCustomProperty(css, name));
    if (reused.length > 0) {
      issues.push({
        kind: "main_stylesheet_reuse",
        detail: `poster CSS must not reuse app tokens: ${reused.join(", ")}`,
      });
    }
  }

  return issues;
}

export function extractCssImports(moduleSource: string): string[] {
  const stripped = moduleSource.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
  const imports: string[] = [];
  for (const match of stripped.matchAll(/import\s+["']([^"']+\.css)["']/g)) {
    const spec = match[1];
    if (spec) {
      imports.push(spec);
    }
  }
  return imports;
}

export function findExportedComponentFile(
  files: readonly { path: string; source: string }[],
  exportName: string,
): string | undefined {
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(exportName)) {
    return undefined;
  }
  const exportRe = new RegExp(`export\\s+(?:function|const)\\s+${exportName}\\b`);
  const matches = files.filter((file) => exportRe.test(file.source)).map((file) => file.path);
  return matches.length === 1 ? matches[0] : undefined;
}
