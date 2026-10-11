import { describe, expect, it } from "vitest";
import { sourceLabel } from "./source";

describe("sourceLabel", () => {
  it("maps slugs to the desktop application names", () => {
    expect(sourceLabel("claude")).toBe("Claude Code");
    expect(sourceLabel("codex")).toBe("Codex");
    expect(sourceLabel("factory")).toBe("Droid");
    expect(sourceLabel("agy")).toBe("Antigravity");
    expect(sourceLabel("copilot")).toBe("GitHub Copilot CLI");
    expect(sourceLabel("cursor_agent")).toBe("Cursor Agent");
  });

  it("leaves unknown slugs alone", () => {
    expect(sourceLabel("brand_new")).toBe("brand_new");
  });
});
