import { describe, expect, it } from "vitest";
import type { OfficialQuotaHistoryPoint } from "../types";
import {
  groupQuotaHistory,
  quotaHistoryOption,
  quotaHistoryProviders,
} from "./quotaHistoryChart";

function point(
  provider: string,
  kind: string,
  capturedAt: string,
  used: number | null,
): OfficialQuotaHistoryPoint {
  return {
    provider,
    window_kind: kind,
    window_label: kind === "weekly" ? "7 天" : "5 小时",
    captured_at: capturedAt,
    used_percent: used,
    used_amount: null,
    limit_amount: null,
    currency: null,
  };
}

describe("groupQuotaHistory", () => {
  it("keeps one series per official window and skips missing percents", () => {
    const series = groupQuotaHistory(
      [
        point("claude", "weekly", "2026-10-01T12:00:00+00:00", 20),
        point("claude", "weekly", "2026-10-02T12:00:00+00:00", 40),
        point("claude", "session_5h", "2026-10-02T12:00:00+00:00", null),
        point("cursor", "weekly", "2026-10-02T12:00:00+00:00", 10),
      ],
      "claude",
    );
    expect(series).toHaveLength(1);
    expect(series[0]?.label).toBe("7 天");
    expect(series[0]?.points).toHaveLength(2);
  });
});

describe("quotaHistoryProviders", () => {
  it("lists unique provider ids in first-seen order", () => {
    expect(
      quotaHistoryProviders([
        point("claude", "weekly", "2026-10-01T12:00:00+00:00", 20),
        point("cursor", "weekly", "2026-10-01T12:00:00+00:00", 10),
        point("claude", "weekly", "2026-10-02T12:00:00+00:00", 30),
      ]),
    ).toEqual(["claude", "cursor"]);
  });
});

describe("quotaHistoryOption", () => {
  it("plots used_percent 0–100 and does not mention local estimate windows", () => {
    const option = quotaHistoryOption(
      groupQuotaHistory(
        [
          point("claude", "weekly", "2026-10-01T12:00:00+00:00", 20),
          point("claude", "weekly", "2026-10-02T12:00:00+00:00", 40),
        ],
        "claude",
      ),
    );
    expect(option.yAxis).toMatchObject({ min: 0, max: 100 });
    expect(JSON.stringify(option)).not.toMatch(/5 小时估计|7 天估计/);
  });
});
