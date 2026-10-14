import { describe, expect, it } from "vitest";
import { unifiedCostSourceLabel } from "./cost";

describe("unifiedCostSourceLabel", () => {
  it("names each pricing layer", () => {
    expect(unifiedCostSourceLabel("native")).toBe("来源自带");
    expect(unifiedCostSourceLabel("team")).toBe("团队价目");
    expect(unifiedCostSourceLabel("snapshot")).toBe("内置快照");
    expect(unifiedCostSourceLabel("none")).toBe("未定价");
    expect(unifiedCostSourceLabel(null)).toBe("未计算");
  });
});
