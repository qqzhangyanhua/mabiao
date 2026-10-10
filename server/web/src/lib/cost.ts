import type { Breakdown } from "../api/types";

/** `unified`：服务端团队价目统一重算，跨人对比用它。`snapshot`：成员本机当时算的，只作对照。 */
export type CostMode = "unified" | "snapshot";

export const COST_MODE_LABEL: Record<CostMode, string> = {
  unified: "统一价",
  snapshot: "客户端快照",
};

export function costOf(
  row: Pick<Breakdown, "unified_cost" | "cost_snapshot">,
  mode: CostMode,
): number {
  return mode === "unified" ? row.unified_cost : row.cost_snapshot;
}
