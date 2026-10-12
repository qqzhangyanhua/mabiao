import { describe, expect, it } from "vitest";
import { resolveSourceIconId } from "./sourceIcon";

describe("resolveSourceIconId", () => {
  it("maps aliases to the same mark as the desktop table", () => {
    expect(resolveSourceIconId("droid")).toBe("factory");
    expect(resolveSourceIconId("antigravity")).toBe("agy");
    expect(resolveSourceIconId("cursor_agent")).toBe("cursor");
    expect(resolveSourceIconId("qoder_cn")).toBe("qoder");
  });

  it("falls back to unknown", () => {
    expect(resolveSourceIconId("brand_new")).toBe("unknown");
  });
});
