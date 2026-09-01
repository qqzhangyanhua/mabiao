import { describe, expect, it } from "vitest";
import { costEstimateKpiLink, unpricedKpiLink } from "./unpricedKpi";

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

describe("costEstimateKpiLink", () => {
  it("explains the estimate when every model is priced", () => {
    const link = costEstimateKpiLink(false);
    expect(link.hint).toContain("估算");
    expect(link.actionLabel).toBeUndefined();
  });

  it("marks the overview cost card as a diagnosis entry when unpriced", () => {
    const link = costEstimateKpiLink(true);
    expect(link.hint).toContain("单价未配置");
    expect(link.actionLabel).toBe("查看全库诊断");
  });
});
