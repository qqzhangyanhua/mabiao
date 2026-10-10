import type {
  AccountView,
  Breakdown,
  CoverageView,
  SessionListItem,
  SummaryResponse,
} from "../api/types";
import type { CsvCell } from "./csv";

const DIMENSIONS: [string, (summary: SummaryResponse) => Breakdown[]][] = [
  ["按天", (s) => s.by_day],
  ["按成员", (s) => s.by_account],
  ["按来源", (s) => s.by_source],
  ["按模型", (s) => s.by_model],
  ["按项目", (s) => s.by_project],
];

/** 当前视图的全部拆分表。两种费用都导出，不随页面上的切换变，方便在表格里自己对照。 */
export function summaryRows(summary: SummaryResponse): CsvCell[][] {
  const rows: CsvCell[][] = [
    ["维度", "名称", "记录数", "token", "统一价费用", "客户端快照费用", "统一价未定价记录数"],
  ];
  for (const [dimension, pick] of DIMENSIONS) {
    for (const item of pick(summary)) {
      rows.push([
        dimension,
        item.label,
        item.record_count,
        item.total_tokens,
        item.unified_cost,
        item.cost_snapshot,
        item.unpriced_count,
      ]);
    }
  }
  return rows;
}

export function sessionRows(sessions: SessionListItem[]): CsvCell[][] {
  return [
    ["成员", "设备", "来源", "标题", "项目", "模型", "开始时间", "结束时间", "事件数", "推送时间"],
    ...sessions.map((s) => [
      s.account,
      s.device_name,
      s.source,
      s.title,
      s.project_name ?? s.project,
      s.model,
      s.started_at,
      s.ended_at,
      s.event_count,
      s.pushed_at,
    ]),
  ];
}

export function memberRows(accounts: AccountView[], coverage: CoverageView[]): CsvCell[][] {
  const byId = new Map(coverage.map((c) => [c.account_id, c]));
  return [
    ["账号", "角色", "状态", "设备数", "最后推送时间", "已覆盖到"],
    ...accounts.map((a) => {
      const c = byId.get(a.id);
      return [
        a.account,
        a.role === "admin" ? "管理员" : "成员",
        a.active ? "正常" : "已停用",
        c?.device_count ?? 0,
        c?.last_push_at ?? null,
        c?.covered_through ?? null,
      ];
    }),
  ];
}
