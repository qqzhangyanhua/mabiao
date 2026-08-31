import { describe, expect, it } from "vitest";
import {
  applicationEfficiencyTable,
  applicationProjectMatrixTable,
  codeVolumeTable,
  cursorAccountDailyTable,
  cursorAccountEventTable,
  cursorAccountModelTable,
  cursorSessionDetailToolTable,
  cursorSessionHashFileTable,
  cursorSessionPathTable,
  cursorSessionProjectTable,
  cursorSessionToolTable,
  trendSeriesTable,
} from "./exportRows";
import type {
  ApplicationAnalyticsDto,
  CodeVolumeSummary,
  CursorAccountUsageDto,
  CursorSessionSummaryDto,
} from "../types";

const analytics: ApplicationAnalyticsDto = {
  summary: {
    total_tokens: 30,
    session_count: 2,
    cache_hit_rate: 0.25,
    average_session_tokens: 15,
    reasoning_share: 0.1,
  },
  by_application: [
    {
      source: "claude",
      application: "Claude Code",
      metrics: {
        total_tokens: 20,
        session_count: 1,
        cache_hit_rate: 0.5,
        average_session_tokens: 20,
        reasoning_share: 0.2,
      },
    },
    {
      source: "codex",
      application: "Codex",
      metrics: {
        total_tokens: 10,
        session_count: 1,
        cache_hit_rate: null,
        average_session_tokens: 10,
        reasoning_share: null,
      },
    },
  ],
  trend: [],
  projects: [
    { project: "/Users/dev/app", total_tokens: 30, values: { claude: 20, codex: 10 } },
  ],
};

describe("trend series table", () => {
  it("exports chronological rows with share and period delta", () => {
    const table = trendSeriesTable([
      {
        bucket: "2026-08-01",
        total_tokens: 100,
        input_tokens: 80,
        output_tokens: 20,
        cache_read_tokens: 6,
        cache_creation_tokens: 4,
        reasoning_tokens: 5,
        cost: 1.2,
      },
      {
        bucket: "2026-08-02",
        total_tokens: 200,
        input_tokens: 150,
        output_tokens: 50,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        cost: null,
      },
    ]);
    expect(table.headers).toEqual([
      "时间",
      "总量",
      "输入",
      "输出",
      "缓存",
      "推理",
      "费用",
      "占总量%",
      "环比%",
    ]);
    expect(table.rows[0]).toEqual(["08-01", 100, 80, 20, 10, 5, 1.2, 33.33, ""]);
    expect(table.rows[1]).toEqual(["08-02", 200, 150, 50, 0, 0, "", 66.67, 100]);
  });
});

describe("application tables", () => {
  it("exports efficiency rows with blank null ratios", () => {
    const table = applicationEfficiencyTable(analytics);
    expect(table.headers[0]).toBe("来源");
    expect(table.rows[1]).toEqual(["Codex", 10, 1, 10, "", ""]);
  });

  it("exports project matrix with application columns", () => {
    const table = applicationProjectMatrixTable(analytics);
    expect(table.headers).toEqual(["项目", "Claude Code", "Codex", "总计"]);
    expect(table.rows[0][0]).toBe("app");
    expect(table.rows[0].slice(1)).toEqual([20, 10, 30]);
  });
});

describe("cursor export tables", () => {
  it("exports code volume summary", () => {
    const data: CodeVolumeSummary = {
      commit_count: 3,
      lines_added: 100,
      lines_deleted: 20,
      net_lines: 80,
      composer_lines_added: 40,
      composer_lines_deleted: 5,
      human_lines_added: 60,
      human_lines_deleted: 2,
      tab_lines_added: 3,
      tab_lines_deleted: 0,
      ai_percentage: 40,
      total_cost: 8,
      cost_unpriced: false,
      cost_per_thousand_ai_lines: 200,
      daily: [],
      by_branch: [],
      commits: [],
    };
    expect(codeVolumeTable(data).rows).toEqual([
      ["提交数", 3, ""],
      ["新增行", 100, ""],
      ["删除行", 20, ""],
      ["净增行", 80, ""],
      ["AI 生成行", 40, ""],
      ["Tab 行", 3, ""],
      ["人工编写行", 60, ""],
      ["AI 占比", 40, ""],
      ["全部来源累计费用", 8, ""],
      ["每千行 AI 代码成本", 200, ""],
    ]);
  });

  it("marks code volume cost cells as unpriced and empty when cost is unknown", () => {
    const data: CodeVolumeSummary = {
      commit_count: 0,
      lines_added: 0,
      lines_deleted: 0,
      net_lines: 0,
      composer_lines_added: 0,
      composer_lines_deleted: 0,
      human_lines_added: 0,
      human_lines_deleted: 0,
      tab_lines_added: 0,
      tab_lines_deleted: 0,
      ai_percentage: null,
      total_cost: null,
      cost_unpriced: true,
      cost_per_thousand_ai_lines: null,
      daily: [],
      by_branch: [],
      commits: [],
    };
    const table = codeVolumeTable(data);
    expect(table.headers).toEqual(["指标", "数值", "未定价"]);
    expect(table.rows.slice(-2)).toEqual([
      ["全部来源累计费用", "", "是"],
      ["每千行 AI 代码成本", "", "是"],
    ]);
  });

  it("exports account daily and model tables", () => {
    const data: CursorAccountUsageDto = {
      as_of: "2026-08-18T00:00:00Z",
      event_count: 2,
      input_tokens: 8,
      output_tokens: 2,
      cache_read_tokens: 0,
      cache_creation_tokens: 0,
      total_tokens: 10,
      daily: [
        {
          bucket: "2026-08-17",
          total_tokens: 10,
          input_tokens: 8,
          output_tokens: 2,
          cache_read_tokens: 0,
          cache_creation_tokens: 0,
          reasoning_tokens: 0,
          cost: null,
        },
      ],
      by_model: [{ name: "gpt-5", total_tokens: 10, share: 1, cost: null, unpriced: false }],
      headless_tokens: 4,
      interactive_tokens: 6,
      headless_share: 0.4,
    };
    expect(cursorAccountDailyTable(data).rows[0]).toEqual(["2026-08-17", 10, 8, 2]);
    expect(cursorAccountModelTable(data).rows[0]).toEqual(["gpt-5", 10, 1]);
  });

  it("exports session project and tool tables", () => {
    const data: CursorSessionSummaryDto = {
      as_of: null,
      session_count: 2,
      turn_count: 5,
      aborted_count: 0,
      user_prompt_count: 3,
      subagent_count: 1,
      error_rate: 0,
      average_turns: 2.5,
      average_tools_per_turn: 1.8,
      write_read_ratio: 0.4,
      active_project_count: 1,
      by_project: [
        {
          name: "/tmp/demo",
          session_count: 2,
          turn_count: 5,
          error_count: 0,
          files_touched: 0,
          last_seen_at: null,
        },
      ],
      by_model: [],
      by_source: [],
      by_extension: [],
      top_tools: [{ name: "read", call_count: 9 }],
      tool_groups: [{ name: "read", call_count: 9 }],
      daily: [],
    };
    expect(cursorSessionProjectTable(data).rows[0]).toEqual(["demo", 2, 5]);
    expect(cursorSessionToolTable(data).rows[0]).toEqual(["read", 9]);
  });

  it("exports session detail paths and hash files", () => {
    const detail = {
      session: {
        session_id: "s1",
        project: "/tmp/demo",
        turn_count: 1,
        success_count: 1,
        error_count: 0,
        aborted_count: 0,
        user_prompt_count: 1,
        subagent_count: 0,
        models: [],
        sources: [],
        tool_call_count: 2,
        first_seen_at: null,
        last_seen_at: null,
        files_touched: 1,
        source_file: "/tmp/s1.jsonl",
      },
      tools: [{ name: "Read", call_count: 2 }],
      hash_files: [{ path: "/tmp/a.rs", extension: "rs", source: "cli" }],
      read_paths: ["/tmp/a.rs"],
      write_paths: ["/tmp/b.rs"],
      transcript_missing: false,
    };
    expect(cursorSessionDetailToolTable(detail).rows[0]).toEqual(["Read", 2]);
    expect(cursorSessionPathTable(detail).rows).toEqual([
      ["读", "/tmp/a.rs"],
      ["写", "/tmp/b.rs"],
    ]);
    expect(cursorSessionHashFileTable(detail).rows[0]).toEqual(["/tmp/a.rs", "rs", "cli"]);
  });

  it("exports account events", () => {
    expect(
      cursorAccountEventTable([
        {
          occurred_at: "2026-08-17T00:00:00Z",
          model: "grok-4.6",
          input_tokens: 8,
          output_tokens: 2,
          cache_read_tokens: 0,
          cache_creation_tokens: 0,
          total_tokens: 10,
          is_headless: true,
        },
      ]).rows[0],
    ).toEqual(["2026-08-17T00:00:00Z", "grok-4.6", 8, 2, 0, 0, 10, "是"]);
  });
});
