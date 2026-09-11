import { describe, expect, it } from "vitest";
import type { WorkNotesPreviewDto } from "../types";
import {
  formatEstimatedSecs,
  workNotesEstimateCopy,
  workNotesPlainText,
  workNotesUsageCopy,
} from "./workNotesCopy";
import type { WorkNotesDto } from "../types";

function preview(overrides: Partial<WorkNotesPreviewDto> = {}): WorkNotesPreviewDto {
  return {
    range_kind: "this_week",
    start_date: "2026-08-17",
    end_date: "2026-08-19",
    session_count: 6,
    skipped_sparse: 0,
    gate: "ok",
    message: "",
    estimated_calls: 7,
    estimated_secs: 60,
    estimated_input_tokens: 1200,
    estimated_cost: 0.12,
    estimated_unpriced: false,
    sessions: [],
    ...overrides,
  };
}

describe("formatEstimatedSecs", () => {
  it("keeps seconds under a minute", () => {
    expect(formatEstimatedSecs(40)).toBe("约 40 秒");
  });

  it("rounds minutes at or above 60 seconds", () => {
    expect(formatEstimatedSecs(90)).toBe("约 2 分钟");
  });
});

describe("workNotesEstimateCopy", () => {
  it("returns null when there are no sessions", () => {
    expect(workNotesEstimateCopy(preview({ session_count: 0 }))).toBeNull();
  });

  it("shows calls, duration and priced cost", () => {
    expect(workNotesEstimateCopy(preview())).toBe(
      "预计 7 次调用、约 1 分钟、约 $0.12（约 1,200 token）",
    );
  });

  it("says unpriced instead of inventing a cost", () => {
    expect(
      workNotesEstimateCopy(preview({ estimated_unpriced: true, estimated_cost: null })),
    ).toBe("预计 7 次调用、约 1 分钟、费用未定价（约 1,200 token）");
  });
});

function dto(overrides: Partial<WorkNotesDto> = {}): WorkNotesDto {
  return {
    range_kind: "this_week",
    start_date: "2026-08-17",
    end_date: "2026-08-19",
    has_data: true,
    skipped_sparse: 0,
    session_count: 1,
    project_count: 1,
    active_days: 1,
    total_tokens: 2000,
    headline: "主线",
    entries: [],
    closing: "",
    extra_instructions: "",
    failed_count: 0,
    failures: [],
    actual_input_tokens: 150,
    actual_output_tokens: 30,
    actual_cost: 0.12,
    actual_unpriced: false,
    ...overrides,
  };
}

describe("workNotesUsageCopy", () => {
  it("shows actual tokens and cost", () => {
    expect(workNotesUsageCopy(dto())).toBe("实际 150 输入 / 30 输出 token，$0.12");
  });

  it("says the engine did not return usage", () => {
    expect(
      workNotesUsageCopy(
        dto({ actual_unpriced: true, actual_input_tokens: 0, actual_output_tokens: 0, actual_cost: null }),
      ),
    ).toBe("本次引擎没有回吐用量");
  });
});

describe("workNotesPlainText", () => {
  it("renders headline, entries and closing", () => {
    const result = workNotesPlainText(
      dto({
        headline: "本周主线",
        entries: [
          { title: "重构缓存层", detail: "降低锁竞争 45%", project: "/home/user/proj/api" },
        ],
        closing: "下周继续优化",
      }),
    );
    expect(result).toContain("本周主线");
    expect(result).toContain("01  重构缓存层");
    expect(result).toContain("    降低锁竞争 45%");
    expect(result).toContain("    📁 api");
    expect(result).toContain("下周继续优化");
  });

  it("omits project line when project is empty", () => {
    const result = workNotesPlainText(
      dto({
        entries: [{ title: "修 bug", detail: "修了一个", project: "" }],
      }),
    );
    expect(result).not.toContain("📁");
  });

  it("appends skipped sparse note when present", () => {
    const result = workNotesPlainText(dto({ skipped_sparse: 3 }));
    expect(result).toContain("已略过 3 个零星会话");
  });

  it("omits skipped note when count is zero", () => {
    const result = workNotesPlainText(dto({ skipped_sparse: 0 }));
    expect(result).not.toContain("零星会话");
  });

  it("handles multiple entries with correct numbering", () => {
    const result = workNotesPlainText(
      dto({
        entries: [
          { title: "A", detail: "", project: "" },
          { title: "B", detail: "", project: "" },
          { title: "C", detail: "", project: "" },
        ],
      }),
    );
    expect(result).toContain("01  A");
    expect(result).toContain("02  B");
    expect(result).toContain("03  C");
  });
});
