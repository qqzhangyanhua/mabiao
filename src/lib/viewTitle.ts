import type { View } from "../types";

const NAV_LABELS: Partial<Record<View, string>> = {
  overview: "概览",
  trend: "使用统计",
  model: "模型统计",
  project: "项目统计",
  application: "来源统计",
  provider: "接口统计",
  worktime: "工作时间线",
  conversations: "对话记录",
  cursor: "代码量",
  "cursor-sessions": "会话",
  instructions: "全局指令",
  settings: "设置",
};

export function navLabel(view: View): string {
  const label = NAV_LABELS[view];
  if (!label) {
    throw new Error(`no nav label for view: ${view}`);
  }
  return label;
}

export function viewTitle(view: View): { title: string; subtitle: string } {
  switch (view) {
    case "overview":
      return { title: "概览", subtitle: "全局 Token 使用概览" };
    case "trend":
      return { title: "使用统计", subtitle: "按时间查看 Token 消耗" };
    case "conversations":
      return { title: "对话记录", subtitle: "全来源正文；Cursor Agent 也在这里" };
    case "model":
      return { title: "模型统计", subtitle: "按模型拆分 Token 与费用" };
    case "project":
      return { title: "项目统计", subtitle: "按项目拆分 Token 与费用" };
    case "application":
      return { title: "来源统计", subtitle: "按本地工具看趋势、交叉与效率" };
    case "provider":
      return { title: "接口统计", subtitle: "按实际调用的 API 拆分，区分官方与中转" };
    case "worktime":
      return { title: "工作时间线", subtitle: "单日会话区间；点横条打开同一条对话记录" };
    case "cursor":
      return { title: "Cursor 代码量", subtitle: "独立口径，不计入 Token" };
    case "cursor-sessions":
      return {
        title: "Cursor 会话",
        subtitle: "跨会话行为聚合，不含正文；点行打开同一条对话记录",
      };
    case "instructions":
      return { title: "全局指令", subtitle: "跨来源的全局指令与体检" };
    case "settings":
      return { title: "设置", subtitle: "外观、数据源与单价" };
  }
}
