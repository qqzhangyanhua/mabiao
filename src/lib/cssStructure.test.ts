import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  countSourceLines,
  inspectStyleLayout,
  parseStyleEntry,
} from "./cssStructure";

describe("parseStyleEntry", () => {
  it("只收下聚合语句和注释", () => {
    const parsed = parseStyleEntry(`/* barrel */
@import "./styles/base/tokens.css";

@import "./styles/layout/shell.css";
`);
    expect(parsed.imports).toEqual([
      "./styles/base/tokens.css",
      "./styles/layout/shell.css",
    ]);
    expect(parsed.issues).toEqual([]);
  });

  it("入口里出现规则就算越界", () => {
    const parsed = parseStyleEntry(`@import "./styles/base/tokens.css";
.foo { color: red; }
`);
    expect(parsed.imports).toEqual(["./styles/base/tokens.css"]);
    expect(parsed.issues).toEqual([{ line: 2, text: ".foo { color: red; }" }]);
  });
});

describe("countSourceLines", () => {
  it("按换行计数，末尾换行不另算一行", () => {
    expect(countSourceLines("")).toBe(0);
    expect(countSourceLines("a\n")).toBe(1);
    expect(countSourceLines("a\nb\n")).toBe(2);
    expect(countSourceLines("a\nb")).toBe(2);
  });
});

describe("inspectStyleLayout", () => {
  it("入口规则、缺失目标和超行都报出来", () => {
    const long = `${"x\n".repeat(401)}`;
    const issues = inspectStyleLayout({
      entrySource: `@import "./styles/base/tokens.css";
.bar { }
@import "./styles/missing.css";
`,
      files: [
        { path: "base/tokens.css", source: ":root {}\n" },
        { path: "too-long.css", source: long },
      ],
      maxLines: 400,
    });
    expect(issues).toEqual([
      { kind: "entry_rule", line: 2, text: ".bar { }" },
      { kind: "missing_import", target: "./styles/missing.css" },
      { kind: "too_long", path: "too-long.css", lines: 401 },
    ]);
  });

  it("干净的入口和未超行文件没有 issue", () => {
    expect(
      inspectStyleLayout({
        entrySource: '@import "./styles/base/tokens.css";\n',
        files: [{ path: "base/tokens.css", source: ":root {\n}\n" }],
      }),
    ).toEqual([]);
  });
});

describe("仓库样式树", () => {
  it("入口只聚合，引用存在，单文件不超过 400 行", () => {
    const entry = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
    const stylesDir = new URL("../styles/", import.meta.url);
    const names = readdirSync(stylesDir, { recursive: true });
    const files = names
      .filter((name) => name.endsWith(".css"))
      .map((name) => {
        const path = name.replaceAll("\\", "/");
        return {
          path,
          source: readFileSync(new URL(path, stylesDir), "utf8"),
        };
      });
    expect(inspectStyleLayout({ entrySource: entry, files })).toEqual([]);
  });
});
