import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { listedSourceTones } from "./chart";
import { listedSourceLabels, sourceLabel } from "./source";
import { listedSourceIconSlugs, resolveSourceIconId } from "./sourceIcon";

describe("sourceLabel", () => {
  it("maps slugs to the desktop application names", () => {
    expect(sourceLabel("claude")).toBe("Claude Code");
    expect(sourceLabel("codex")).toBe("Codex");
    expect(sourceLabel("factory")).toBe("Droid");
    expect(sourceLabel("agy")).toBe("Antigravity");
    expect(sourceLabel("copilot")).toBe("GitHub Copilot CLI");
    expect(sourceLabel("cursor_agent")).toBe("Cursor Agent");
    expect(sourceLabel("codebuddy_ide")).toBe("CodeBuddy IDE");
    expect(sourceLabel("windsurf")).toBe("Windsurf");
  });

  it("leaves unknown slugs alone", () => {
    expect(sourceLabel("brand_new")).toBe("brand_new");
  });
});

describe("desktop source identity coverage", () => {
  it("covers every desktop SOURCE_SLUGS entry with label, icon and tone", () => {
    const desktopSlugs = readDesktopSourceSlugs();
    expect(desktopSlugs.length).toBe(35);
    const labels = listedSourceLabels();
    const icons = listedSourceIconSlugs();
    const tones = listedSourceTones();
    expect(desktopSlugs.filter((slug) => !labels.has(slug))).toEqual([]);
    expect(desktopSlugs.filter((slug) => !icons.has(slug))).toEqual([]);
    expect(desktopSlugs.filter((slug) => !tones.has(slug))).toEqual([]);
    for (const slug of desktopSlugs) {
      expect(resolveSourceIconId(slug)).not.toBe("unknown");
    }
  });
});

function readDesktopSourceSlugs(): string[] {
  const here = dirname(fileURLToPath(import.meta.url));
  const candidates = [
    resolve(here, "../../../../src/generated/displayIdentity.ts"),
    resolve(here, "../../../../../src/generated/displayIdentity.ts"),
  ];
  const identity = candidates.find((candidate) => existsSync(candidate));
  if (!identity) {
    throw new Error("desktop displayIdentity.ts not found beside server/web");
  }
  const text = readFileSync(identity, "utf8");
  const match = /export const SOURCE_SLUGS = \[([\s\S]*?)\] as const;/.exec(text);
  if (!match) {
    throw new Error("SOURCE_SLUGS not found in desktop displayIdentity.ts");
  }
  const body = match[1] ?? "";
  return [...body.matchAll(/"([^"]+)"/g)].flatMap((entry) => {
    const slug = entry[1];
    return slug === undefined ? [] : [slug];
  });
}
