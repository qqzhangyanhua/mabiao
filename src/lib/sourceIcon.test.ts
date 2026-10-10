import { describe, expect, it } from "vitest";
import { resolveSourceIconId } from "./sourceIcon";

const KNOWN_SOURCES = [
  "claude",
  "codex",
  "pi",
  "omp",
  "opencode",
  "kimi",
  "dsh",
  "gemini",
  "grok",
  "qwen",
  "factory",
  "cursor",
  "cursor_agent",
  "copilot",
  "hermes",
  "agy",
  "qoder",
  "qoder_cn",
  "cline",
  "workbuddy",
  "zcode",
] as const;

describe("resolveSourceIconId", () => {
  it("gives every known application source a dedicated glyph id", () => {
    for (const source of KNOWN_SOURCES) {
      expect(resolveSourceIconId(source)).not.toBe("unknown");
    }
  });

  it("maps each product id to itself", () => {
    expect(resolveSourceIconId("claude")).toBe("claude");
    expect(resolveSourceIconId("codex")).toBe("codex");
    expect(resolveSourceIconId("grok")).toBe("grok");
    expect(resolveSourceIconId("copilot")).toBe("copilot");
    expect(resolveSourceIconId("opencode")).toBe("opencode");
    expect(resolveSourceIconId("omp")).toBe("omp");
    expect(resolveSourceIconId("factory")).toBe("factory");
    expect(resolveSourceIconId("hermes")).toBe("hermes");
    expect(resolveSourceIconId("qoder")).toBe("qoder");
    expect(resolveSourceIconId("qoder_cn")).toBe("qoder");
    expect(resolveSourceIconId("cline")).toBe("cline");
    expect(resolveSourceIconId("workbuddy")).toBe("workbuddy");
    expect(resolveSourceIconId("zcode")).toBe("zcode");
  });

  it("uses one Cursor face for Cursor and Cursor Agent", () => {
    expect(resolveSourceIconId("cursor")).toBe("cursor");
    expect(resolveSourceIconId("cursor_agent")).toBe("cursor");
  });

  it("uses the Droid face for factory and official quota droid ids", () => {
    expect(resolveSourceIconId("factory")).toBe("factory");
    expect(resolveSourceIconId("droid")).toBe("factory");
  });

  it("uses one Qoder face for Qoder and Qoder CN", () => {
    expect(resolveSourceIconId("qoder")).toBe("qoder");
    expect(resolveSourceIconId("qoder_cn")).toBe("qoder");
  });

  it("uses the Gemini face for Antigravity official quota and agy usage", () => {
    expect(resolveSourceIconId("antigravity")).toBe("gemini");
    expect(resolveSourceIconId("agy")).toBe("gemini");
  });

  it("falls back to the generic mark instead of a first letter", () => {
    expect(resolveSourceIconId("some_new_source")).toBe("unknown");
    expect(resolveSourceIconId("")).toBe("unknown");
    expect(resolveSourceIconId("Claude")).toBe("unknown");
  });
});
