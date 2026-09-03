/** 样式入口与分层文件的结构门禁：入口只聚合，单文件有行数上限。 */

export const MAX_STYLE_FILE_LINES = 400;

export type StyleEntryIssue = {
  line: number;
  text: string;
};

export function parseStyleEntry(source: string): {
  imports: string[];
  issues: StyleEntryIssue[];
} {
  const imports: string[] = [];
  const issues: StyleEntryIssue[] = [];
  const withoutComments = source.replace(/\/\*[\s\S]*?\*\//g, (block) =>
    block.replace(/[^\n]/g, " "),
  );
  for (const [index, line] of withoutComments.split("\n").entries()) {
    const text = line.trim();
    if (!text) {
      continue;
    }
    const match = /^@import\s+"([^"]+)";$/.exec(text);
    if (match) {
      imports.push(match[1]);
      continue;
    }
    issues.push({ line: index + 1, text });
  }
  return { imports, issues };
}

export function countSourceLines(source: string): number {
  if (source.length === 0) {
    return 0;
  }
  const parts = source.split("\n");
  return source.endsWith("\n") ? parts.length - 1 : parts.length;
}

export type StyleLayoutIssue =
  | { kind: "entry_rule"; line: number; text: string }
  | { kind: "missing_import"; target: string }
  | { kind: "too_long"; path: string; lines: number };

export function inspectStyleLayout({
  entrySource,
  files,
  maxLines = MAX_STYLE_FILE_LINES,
}: {
  entrySource: string;
  files: { path: string; source: string }[];
  maxLines?: number;
}): StyleLayoutIssue[] {
  const issues: StyleLayoutIssue[] = [];
  const parsed = parseStyleEntry(entrySource);
  for (const issue of parsed.issues) {
    issues.push({ kind: "entry_rule", line: issue.line, text: issue.text });
  }
  const available = new Set(files.map((file) => file.path.replaceAll("\\", "/")));
  for (const target of parsed.imports) {
    const relative = target.replace(/^\.\//, "").replace(/^styles\//, "");
    if (!available.has(relative)) {
      issues.push({ kind: "missing_import", target });
    }
  }
  for (const file of files) {
    const lines = countSourceLines(file.source);
    if (lines > maxLines) {
      issues.push({ kind: "too_long", path: file.path, lines });
    }
  }
  return issues;
}
