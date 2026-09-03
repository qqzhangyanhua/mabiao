import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const CSS_FILES = ["poster.css", "quotaPoster.css", "spike.css"] as const;

describe("report poster CSS subset", () => {
  it("does not use color-mix or backdrop-filter", () => {
    for (const name of CSS_FILES) {
      const css = readFileSync(new URL(name, import.meta.url), "utf8");
      expect(css.length, name).toBeGreaterThan(0);
      expect(css, name).not.toMatch(/color-mix\s*\(/i);
      expect(css, name).not.toMatch(/backdrop-filter/i);
    }
  });
});
