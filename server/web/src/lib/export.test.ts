import { describe, expect, it } from "vitest";
import type { AccountView, CoverageView, SummaryResponse } from "../api/types";
import { memberRows, summaryRows } from "./export";

const row = (key: string, unified: number, snapshot: number) => ({
  key,
  label: key,
  id: null,
  record_count: 2,
  total_tokens: 3000,
  cost_snapshot: snapshot,
  unified_cost: unified,
  unpriced_count: 1,
});

describe("summaryRows", () => {
  it("lists every dimension with both costs, whatever the page toggle says", () => {
    const summary: SummaryResponse = {
      totals: {
        record_count: 2,
        total_tokens: 3000,
        cost_snapshot_total: 9,
        unified_cost_total: 1,
        unified_unpriced_count: 1,
      },
      by_day: [row("2026-03-01", 1, 9)],
      by_account: [row("alice", 1, 9)],
      by_source: [],
      by_model: [row("gpt-5", 1, 9)],
      by_project: [],
    };
    const rows = summaryRows(summary);
    expect(rows[0]).toEqual([
      "维度",
      "名称",
      "记录数",
      "token",
      "统一价费用",
      "客户端快照费用",
      "统一价未定价记录数",
    ]);
    expect(rows.slice(1).map((r) => r[0])).toEqual(["按天", "按成员", "按模型"]);
    expect(rows[1]).toEqual(["按天", "2026-03-01", 2, 3000, 1, 9, 1]);
  });
});

describe("memberRows", () => {
  it("joins accounts with their coverage and tolerates members who never pushed", () => {
    const accounts: AccountView[] = [
      { id: 1, account: "root", role: "admin", active: true, created_at: "", deactivated_at: null },
      {
        id: 2,
        account: "alice",
        role: "member",
        active: false,
        created_at: "",
        deactivated_at: "",
      },
    ];
    const coverage: CoverageView[] = [
      {
        account_id: 2,
        account: "alice",
        active: false,
        device_count: 2,
        last_push_at: "2026-03-02T00:00:00Z",
        covered_through: "2026-03-01",
      },
    ];
    expect(memberRows(accounts, coverage)).toEqual([
      ["账号", "角色", "状态", "设备数", "最后推送时间", "已覆盖到"],
      ["root", "管理员", "正常", 0, null, null],
      ["alice", "成员", "已停用", 2, "2026-03-02T00:00:00Z", "2026-03-01"],
    ]);
  });
});
