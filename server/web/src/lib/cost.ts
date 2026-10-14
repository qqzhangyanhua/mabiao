import type { Breakdown } from "../api/types";

/** `unified`：服务端团队价目统一重算，跨人对比用它。`snapshot`：成员本机当时算的，只作对照。 */
export type CostMode = "unified" | "snapshot";

export const COST_MODE_LABEL: Record<CostMode, string> = {
  unified: "统一价",
  snapshot: "客户端快照",
};

export type UnifiedCostSource = "native" | "team" | "snapshot" | "none";

export const UNIFIED_COST_SOURCE_LABEL: Record<UnifiedCostSource, string> = {
  native: "来源自带",
  team: "团队价目",
  snapshot: "内置快照",
  none: "未定价",
};

export function unifiedCostSourceLabel(value: string | null): string {
  if (!value) return "未计算";
  if (value in UNIFIED_COST_SOURCE_LABEL) {
    return UNIFIED_COST_SOURCE_LABEL[value as UnifiedCostSource];
  }
  return value;
}

export function costOf(
  row: Pick<Breakdown, "unified_cost" | "cost_snapshot">,
  mode: CostMode,
): number {
  return mode === "unified" ? row.unified_cost : row.cost_snapshot;
}
