import { describe, expect, it } from "vitest";
import type { WorkNotesDto } from "../types";
import { toWorkNotesPosterViewModel } from "./workNotesPoster";

function dto(overrides: Partial<WorkNotesDto> = {}): WorkNotesDto {
  return {
    range_kind: "this_week",
    start_date: "2026-08-17",
    end_date: "2026-08-19",
    has_data: true,
    skipped_sparse: 0,
    session_count: 12,
    project_count: 4,
    active_days: 5,
    total_tokens: 1_250_000,
    headline: "把会话目录收成一张能发出去的图",
    entries: [
      {
        title: "纪要海报",
        detail: "条目随内容撑高，不硬凑五条。",
        project: "statistics",
      },
    ],
    closing: "下周把分享入口也接上。",
    extra_instructions: "",
    failed_count: 0,
    failures: [],
    actual_input_tokens: 0,
    actual_output_tokens: 0,
    actual_cost: null,
    actual_unpriced: true,
    ...overrides,
  };
}

describe("toWorkNotesPosterViewModel", () => {
  it("returns null when the range has no work notes", () => {
    expect(toWorkNotesPosterViewModel(dto({ has_data: false, headline: "不该出现" }))).toBeNull();
  });

  it("maps this-week kicker, token unit, hard numbers, and copy", () => {
    const poster = toWorkNotesPosterViewModel(dto());
    expect(poster).toEqual({
      kicker: "码表 · 本周纪要",
      rangeLabel: "2026年8月17日 – 8月19日",
      headline: "把会话目录收成一张能发出去的图",
      closing: "下周把分享入口也接上。",
      entries: [
        {
          title: "纪要海报",
          detail: "条目随内容撑高，不硬凑五条。",
          project: "statistics",
        },
      ],
      metrics: [
        { id: "sessions", label: "会话", value: "12" },
        { id: "projects", label: "项目", value: "4" },
        { id: "active_days", label: "活跃天数", value: "5" },
        { id: "tokens", label: "本周 token", value: "1.25M" },
      ],
      skippedLabel: null,
    });
  });

  it("changes kicker and token unit for today, this month and custom ranges", () => {
    const today = toWorkNotesPosterViewModel(
      dto({
        range_kind: "today",
        start_date: "2026-08-19",
        end_date: "2026-08-19",
      }),
    );
    expect(today?.kicker).toBe("码表 · 今日纪要");
    expect(today?.metrics.find((metric) => metric.id === "tokens")?.label).toBe("今日 token");

    const month = toWorkNotesPosterViewModel(dto({ range_kind: "this_month" }));
    expect(month?.kicker).toBe("码表 · 本月纪要");
    expect(month?.metrics.find((metric) => metric.id === "tokens")?.label).toBe("本月 token");

    const custom = toWorkNotesPosterViewModel(
      dto({
        range_kind: "custom",
        start_date: "2026-07-01",
        end_date: "2026-07-31",
      }),
    );
    expect(custom?.kicker).toBe("码表 · 区间纪要");
    expect(custom?.rangeLabel).toBe("2026年7月1日 – 7月31日");
    expect(custom?.metrics.find((metric) => metric.id === "tokens")?.label).toBe("区间 token");
  });

  it("writes skipped sparse sessions instead of dropping them silently", () => {
    expect(toWorkNotesPosterViewModel(dto({ skipped_sparse: 3 }))?.skippedLabel).toBe(
      "已略过 3 个零星会话",
    );
    expect(toWorkNotesPosterViewModel(dto({ skipped_sparse: 0 }))?.skippedLabel).toBeNull();
  });

  it("keeps only the directory name when a project path leaks through", () => {
    const poster = toWorkNotesPosterViewModel(
      dto({
        entries: [
          {
            title: "路径",
            detail: "截图不能带用户名。",
            project: "/Users/alice/work/statistics",
          },
          {
            title: "空项目",
            detail: "没有目录就不写。",
            project: "  ",
          },
          {
            title: "Windows",
            detail: "反斜杠同样只留末段。",
            project: "C:\\Users\\alice\\project",
          },
        ],
      }),
    );
    expect(poster?.entries.map((entry) => entry.project)).toEqual(["statistics", null, "project"]);
    expect(JSON.stringify(poster)).not.toContain("/Users/");
    expect(JSON.stringify(poster)).not.toContain("C:\\\\Users");
  });

  it("does not carry report poster slots or engine usage onto the work notes card", () => {
    const poster = toWorkNotesPosterViewModel(dto());
    expect(poster).not.toBeNull();
    const payload = JSON.stringify(poster);
    expect(poster).not.toHaveProperty("days");
    expect(poster).not.toHaveProperty("sources");
    expect(poster).not.toHaveProperty("stats");
    expect(poster).not.toHaveProperty("totalCostLabel");
    expect(poster).not.toHaveProperty("comments");
    expect(payload).not.toContain("码表 · 周报");
    expect(payload).not.toContain("按天节奏");
    expect(payload).not.toContain("来源占比");
    expect(payload).not.toContain("你这周烧掉了");
    expect(payload).not.toContain("$0.12");
    expect(payload).not.toContain("实际 150");
  });

  it("lets entry count float with the dto instead of padding to a fixed length", () => {
    const three = toWorkNotesPosterViewModel(
      dto({
        entries: [
          { title: "一", detail: "甲", project: "a" },
          { title: "二", detail: "乙", project: "b" },
          { title: "三", detail: "丙", project: "c" },
        ],
      }),
    );
    const six = toWorkNotesPosterViewModel(
      dto({
        entries: [
          { title: "一", detail: "甲", project: "a" },
          { title: "二", detail: "乙", project: "b" },
          { title: "三", detail: "丙", project: "c" },
          { title: "四", detail: "丁", project: "d" },
          { title: "五", detail: "戊", project: "e" },
          { title: "六", detail: "己", project: "f" },
        ],
      }),
    );
    expect(three?.entries).toHaveLength(3);
    expect(six?.entries).toHaveLength(6);
  });
});
