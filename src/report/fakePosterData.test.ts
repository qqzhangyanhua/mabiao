import { describe, expect, it } from "vitest";
import { EXTREME_REPORT_CASES } from "../lib/reportExtremeFixtures";
import { EXTREME_POSTERS, FAKE_POSTER } from "./fakePosterData";
import { posterNarrativeLines } from "./posterTypes";

describe("fake poster fixtures", () => {
  it("keeps the capture dummy populated across the seven-slot view model", () => {
    expect(FAKE_POSTER.kicker).toBe("码表 · 周报");
    expect(FAKE_POSTER.rangeLabel.length).toBeGreaterThan(0);
    expect(FAKE_POSTER.totalTokensLabel.length).toBeGreaterThan(0);
    expect(FAKE_POSTER.totalUnit).toBe("本周 token");
    expect(FAKE_POSTER.comments.length).toBeGreaterThan(0);
    expect(FAKE_POSTER.days).toHaveLength(7);
    expect(FAKE_POSTER.sources.length).toBeGreaterThan(0);
    expect(FAKE_POSTER.stats).toHaveLength(3);
    expect(FAKE_POSTER.stats.map((stat) => stat.label)).toEqual([
      "最忙的一天",
      "模型 Top 3",
      "最贵的一次",
    ]);
    expect(FAKE_POSTER.cursorAccount).toBeNull();
  });

  it("appends the Cursor account line after existing comments", () => {
    expect(posterNarrativeLines(FAKE_POSTER)).toEqual(FAKE_POSTER.comments);
    expect(
      posterNarrativeLines({
        ...FAKE_POSTER,
        cursorAccount: {
          tokensLabel: "1.2M",
          costLabel: null,
          modelsLabel: "gpt-5",
          note: "云端账号，不并入上方本机总量",
          line: "Cursor 账号另有 1.2M token，云端口径，未并入上方。",
        },
      }),
    ).toEqual([...FAKE_POSTER.comments, "Cursor 账号另有 1.2M token，云端口径，未并入上方。"]);
  });

  it("maps every extreme DTO onto a seven-slot poster view model", () => {
    expect(EXTREME_POSTERS.map((item) => item.id)).toEqual(EXTREME_REPORT_CASES.map((item) => item.id));
    for (const item of EXTREME_POSTERS) {
      expect(item.data.kicker).toBe("码表 · 周报");
      expect(item.data.totalUnit).toBe("本周 token");
      expect(item.data.comments).toHaveLength(3);
      expect(item.data.days).toHaveLength(7);
      expect(item.data.sources.length).toBeGreaterThan(0);
      expect(item.data.stats).toHaveLength(3);
      expect(item.data.stats.find((stat) => stat.kind === "busiest_day")?.label).toBe("最忙的一天");
      expect(item.data.stats.find((stat) => stat.kind === "models")?.label).toMatch(
        /^模型( Top [23])?$/,
      );
      expect(item.data.stats.find((stat) => stat.kind === "top_session")?.label).toMatch(
        /^(最贵的一次|消耗最多的一次)$/,
      );
    }
  });
});
