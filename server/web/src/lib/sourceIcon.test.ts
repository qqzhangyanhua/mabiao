import { describe, expect, it } from "vitest";
import { resolveSourceIconId } from "./sourceIcon";

describe("resolveSourceIconId", () => {
  it("maps aliases to the same brand mark as desktop", () => {
    expect(resolveSourceIconId("droid")).toBe("factory");
    expect(resolveSourceIconId("antigravity")).toBe("agy");
    expect(resolveSourceIconId("cursor_agent")).toBe("cursor");
    expect(resolveSourceIconId("qoder_cn")).toBe("qoder");
    expect(resolveSourceIconId("codebuddy_ide")).toBe("codebuddy");
    expect(resolveSourceIconId("windsurf")).toBe("windsurf");
  });

  it("falls back to unknown for brandless or unrecognized slugs", () => {
    expect(resolveSourceIconId("devin")).toBe("unknown");
    expect(resolveSourceIconId("brand_new")).toBe("unknown");
  });
});
