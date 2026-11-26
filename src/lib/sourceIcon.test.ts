import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { BRAND_IDS } from "../generated/displayIdentity";
import { officialQuotaAttributionId } from "./overviewLayout";
import { brandToneClass, resolveSourceIconId } from "./sourceIcon";

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
  "alma",
  "codebuddy",
  "codebuddy_ide",
  "trae",
  "goose",
  "roo",
  "kilo",
  "amp",
  "zed",
  "cherry",
  "openclaw",
  "crush",
  "aider",
  "warp",
  "windsurf",
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
    expect(resolveSourceIconId("alma")).toBe("alma");
    expect(resolveSourceIconId("codebuddy")).toBe("codebuddy");
    expect(resolveSourceIconId("trae")).toBe("trae");
    expect(resolveSourceIconId("goose")).toBe("goose");
    expect(resolveSourceIconId("roo")).toBe("roo");
    expect(resolveSourceIconId("kilo")).toBe("kilo");
    expect(resolveSourceIconId("amp")).toBe("amp");
    expect(resolveSourceIconId("zed")).toBe("zed");
    expect(resolveSourceIconId("cherry")).toBe("cherry");
    expect(resolveSourceIconId("openclaw")).toBe("openclaw");
    expect(resolveSourceIconId("crush")).toBe("crush");
    expect(resolveSourceIconId("aider")).toBe("aider");
    expect(resolveSourceIconId("warp")).toBe("warp");
    expect(resolveSourceIconId("windsurf")).toBe("windsurf");
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

  it("uses the Antigravity face for official quota and agy usage", () => {
    expect(resolveSourceIconId("antigravity")).toBe("agy");
    expect(resolveSourceIconId("agy")).toBe("agy");
  });

  it("uses the built-in face for a quota-account row", () => {
    const face = (row: { provider: string; base_provider?: string | null }) => {
      const iconId = resolveSourceIconId(officialQuotaAttributionId(row));
      return {
        iconId,
        tone: iconId === "unknown" ? "tone-other" : brandToneClass(iconId),
      };
    };
    expect(face({ provider: "acct:droid:ab12" })).toEqual({
      iconId: "factory",
      tone: "tone-factory",
    });
    expect(face({ provider: "acct:antigravity:ab12" })).toEqual({
      iconId: "agy",
      tone: "tone-gemini",
    });
    expect(face({ provider: "acct:devin:ab12" })).toEqual({
      iconId: "unknown",
      tone: "tone-other",
    });
    expect(face({ provider: "acct:copilot:ab12" })).toEqual({
      iconId: "copilot",
      tone: "tone-copilot",
    });
    expect(face({ provider: "acct:codex:ab12", base_provider: "claude" })).toEqual({
      iconId: "claude",
      tone: "tone-claude",
    });
    expect(resolveSourceIconId("acct:codex:ab12")).toBe("unknown");
  });

  it("falls back to the generic mark instead of a first letter", () => {
    expect(resolveSourceIconId("some_new_source")).toBe("unknown");
    expect(resolveSourceIconId("")).toBe("unknown");
    expect(resolveSourceIconId("Claude")).toBe("unknown");
  });
});

function toneSelectorExists(css: string, tone: string): boolean {
  return new RegExp(`(?:^|[\\s,{])\\.${tone}(?![\\w-])`, "m").test(css);
}

describe("brandToneClass", () => {
  it("uses a tone class that exists in the tone stylesheet", () => {
    const css = readFileSync(new URL("../styles/shared/tones.css", import.meta.url), "utf8");
    for (const brand of BRAND_IDS) {
      const tone = brandToneClass(brand);
      expect(toneSelectorExists(css, tone), `${brand} -> ${tone}`).toBe(true);
    }
  });
});
