import { describe, expect, it } from "vitest";
import { formatCostBucketLine, formatCostSourceLine } from "./costBreakdown";

describe("formatCostBucketLine", () => {
  it("keeps native as an unsplittable lump and lists priced buckets", () => {
    const line = formatCostBucketLine(
      { input: 1, output: 2, cache_read: 0.003, cache_creation: 0.15 },
      4.2,
    );
    expect(line).toBe(
      "来源自带 $4.20，按口径拆不开 · 输入 $1.00 · 输出 $2.00 · 缓存读 $0.0030 · 缓存写 $0.15",
    );
  });

  it("is only the native note when priced buckets are empty", () => {
    expect(
      formatCostBucketLine(
        { input: null, output: null, cache_read: null, cache_creation: null },
        1.25,
      ),
    ).toBe("来源自带 $1.25，按口径拆不开");
  });

  it("skips zeros and returns null when nothing to show", () => {
    expect(
      formatCostBucketLine(
        { input: 0, output: null, cache_read: 0, cache_creation: null },
        0,
      ),
    ).toBeNull();
  });
});

describe("formatCostSourceLine", () => {
  it("joins amount shares and unpriced record count in one sentence", () => {
    expect(
      formatCostSourceLine({
        native: 2.5,
        user: 2.5,
        snapshot: 5,
        unpriced_records: 3,
      }),
    ).toBe("来源自带 $2.50（25%） · 用户单价 $2.50（25%） · LiteLLM 快照 $5.00（50%） · 未配置 3 条");
  });

  it("omits empty priced sources", () => {
    expect(
      formatCostSourceLine({
        native: null,
        user: 1,
        snapshot: 0,
        unpriced_records: 0,
      }),
    ).toBe("用户单价 $1.00（100%）");
  });

  it("can be only the unpriced count", () => {
    expect(
      formatCostSourceLine({
        native: null,
        user: null,
        snapshot: null,
        unpriced_records: 2,
      }),
    ).toBe("未配置 2 条");
  });
});
