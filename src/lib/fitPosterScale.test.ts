import { describe, expect, it } from "vitest";
import { fitPosterScale } from "./fitPosterScale";

describe("fitPosterScale", () => {
  it("keeps scale at 1 when the poster already fits", () => {
    expect(fitPosterScale(800, 900, 720, 800)).toBe(1);
  });

  it("does not upscale a smaller poster", () => {
    expect(fitPosterScale(1400, 1600, 720, 1000)).toBe(1);
  });

  it("scales down to the tighter side", () => {
    expect(fitPosterScale(720, 500, 720, 1000)).toBe(0.5);
    expect(fitPosterScale(360, 1000, 720, 1000)).toBe(0.5);
  });

  it("uses height when a tall poster would clip", () => {
    expect(fitPosterScale(792, 812, 720, 1000)).toBeCloseTo(0.812, 5);
  });

  it("returns 1 for invalid sizes", () => {
    expect(fitPosterScale(0, 800, 720, 1000)).toBe(1);
    expect(fitPosterScale(800, 800, 0, 1000)).toBe(1);
    expect(fitPosterScale(-1, 800, 720, 1000)).toBe(1);
  });
});
