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
