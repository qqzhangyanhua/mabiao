export type UnpricedKpiLink = {
  hint: string;
  actionLabel: string;
};

/** 拆分页 KPI 跟筛选；诊断清单是全库。筛选下为零时不要画成可点待办。 */
export function unpricedKpiLink(count: number): UnpricedKpiLink | null {
  if (count <= 0) {
    return null;
  }
  return {
    hint: "当前筛选口径；清单是全库，数量可能不同",
    actionLabel: "查看全库诊断",
  };
}

export type CostEstimateKpiLink = {
  hint: string;
  actionLabel?: string;
};

/** 概览总费用是估算。未定价时卡片可点进全库诊断；已定价只解释口径。 */
export function costEstimateKpiLink(unpriced: boolean): CostEstimateKpiLink {
  if (!unpriced) {
    return { hint: "按价目估算，非官方账单" };
  }
  return {
    hint: "按价目估算；部分模型单价未配置，数字可能偏低",
    actionLabel: "查看全库诊断",
  };
}
