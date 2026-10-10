import { describe, expect, it } from "vitest";
import { coverageStatus } from "./coverage";

const now = new Date("2026-03-10T08:00:00Z");

describe("coverageStatus", () => {
  it("flags members who never pushed", () => {
    expect(coverageStatus({ covered_through: null }, now)).toBe("never");
  });

  it("tolerates a short lag but flags a gap longer than three days", () => {
    expect(coverageStatus({ covered_through: "2026-03-10" }, now)).toBe("ok");
    expect(coverageStatus({ covered_through: "2026-03-07" }, now)).toBe("ok");
    expect(coverageStatus({ covered_through: "2026-03-06" }, now)).toBe("stale");
  });

  it("treats an unparseable date as never pushed rather than crashing", () => {
    expect(coverageStatus({ covered_through: "garbage" }, now)).toBe("never");
  });
});
