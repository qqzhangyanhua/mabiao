import { describe, expect, it } from "vitest";
import { addSplit, cacheHitRate, emptySplit, splitSum } from "./tokens";

describe("cacheHitRate", () => {
  it("is cache_read over input plus cache_read", () => {
    expect(
      cacheHitRate({
        input: 70,
        output: 10,
        cache_read: 30,
        cache_creation: 5,
        reasoning: 0,
      }),
    ).toBe(0.3);
  });

  it("is null when there is no input and no cache read", () => {
    expect(cacheHitRate(emptySplit())).toBeNull();
  });
});

describe("splitSum / addSplit", () => {
  it("adds the five token dimensions", () => {
    const left = { input: 1, output: 2, cache_read: 3, cache_creation: 4, reasoning: 5 };
    expect(splitSum(addSplit(left, left))).toBe(30);
  });
});
