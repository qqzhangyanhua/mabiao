import { describe, expect, it } from "vitest";
import { unpricedKpiLink } from "./unpricedKpi";

describe("unpricedKpiLink", () => {
  it("keeps a zero count as a static fact, not a clickable todo", () => {
    expect(unpricedKpiLink(0)).toBeNull();
  });

  it("exposes a whole-database diagnosis entry when the filtered count is positive", () => {
    const link = unpricedKpiLink(3);
    expect(link).not.toBeNull();
    expect(link?.actionLabel).toBe("查看全库诊断");
    expect(link?.hint).toContain("全库");
    expect(link?.hint).toContain("筛选");
  });
});
