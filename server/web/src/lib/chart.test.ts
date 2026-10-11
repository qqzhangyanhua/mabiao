import { describe, expect, it } from "vitest";
import { sliceColor, sourceTone } from "./chart";

describe("sourceTone", () => {
  it("maps known sources onto the existing tone tokens", () => {
    expect(sourceTone("codex")).toBe("var(--tone-codex)");
    expect(sourceTone("claude")).toBe("var(--tone-claude)");
    expect(sourceTone("cursor_agent")).toBe("var(--tone-cursor)");
    expect(sourceTone("factory")).toBe("var(--tone-codex)");
  });

  it("falls back to the cursor tone for unknown sources", () => {
    expect(sourceTone("brand_new")).toBe("var(--tone-cursor)");
  });
});

describe("sliceColor", () => {
  it("prefers the source tone, then the chart palette", () => {
    expect(sliceColor("grok", 0)).toBe("var(--tone-grok)");
    expect(sliceColor("unknown-model", 2)).toBe("var(--chart-3)");
  });
});
