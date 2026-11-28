import { describe, expect, it } from "vitest";
import { SOURCE_SLUGS } from "../generated/displayIdentity";
import { sourceTone } from "./chart";
import { sourceLabel } from "./source";
import { resolveSourceIconId } from "./sourceIcon";

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

describe("generated source identity coverage", () => {
  it("resolves label, brand icon, and hex chart tone for every generated source", () => {
    expect(SOURCE_SLUGS.length).toBeGreaterThanOrEqual(35);
    for (const slug of SOURCE_SLUGS) {
      expect(sourceLabel(slug).length).toBeGreaterThan(0);
      expect(resolveSourceIconId(slug)).not.toBe("unknown");
      expect(sourceTone(slug)).toMatch(/^#[0-9a-f]{6}$/);
    }
  });
});
