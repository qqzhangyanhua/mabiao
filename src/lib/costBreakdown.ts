import type { OverviewCostBreakdown, OverviewCostSources } from "../types";

const ZERO = 5e-5;

export function formatUsdAmount(n: number): string {
  if (Math.abs(n) >= 0.01) {
    return `$${n.toFixed(2)}`;
  }
  return `$${n.toFixed(4)}`;
}

function skipAmount(n: number | null): n is number {
  return n != null && Math.abs(n) >= ZERO;
}

/** 费用卡下的四档金额。来源自带整笔拆不开，单独成句。 */
export function formatCostBucketLine(
  breakdown: OverviewCostBreakdown,
  native: number | null,
): string | null {
  const parts: string[] = [];
  if (skipAmount(native)) {
    parts.push(`来源自带 ${formatUsdAmount(native)}，按口径拆不开`);
  }
  const buckets: [string, number | null][] = [
    ["输入", breakdown.input],
    ["输出", breakdown.output],
    ["缓存读", breakdown.cache_read],
    ["缓存写", breakdown.cache_creation],
  ];
  for (const [label, amount] of buckets) {
    if (!skipAmount(amount)) {
      continue;
    }
    parts.push(`${label} ${formatUsdAmount(amount)}`);
  }
  return parts.length > 0 ? parts.join(" · ") : null;
}

/** 费用旁一句话：已计价三档按金额占比，未配置按记录数。 */
export function formatCostSourceLine(sources: OverviewCostSources): string | null {
  const priced: [string, number | null][] = [
    ["来源自带", sources.native],
    ["用户单价", sources.user],
    ["LiteLLM 快照", sources.snapshot],
  ];
  const total = priced.reduce((sum, [, amount]) => sum + (amount ?? 0), 0);
  const parts: string[] = [];
  for (const [label, amount] of priced) {
    if (!skipAmount(amount)) {
      continue;
    }
    const pct = total > 0 ? `（${((amount / total) * 100).toFixed(0)}%）` : "";
    parts.push(`${label} ${formatUsdAmount(amount)}${pct}`);
  }
  if (sources.unpriced_records > 0) {
    parts.push(`未配置 ${sources.unpriced_records.toLocaleString("zh-CN")} 条`);
  }
  return parts.length > 0 ? parts.join(" · ") : null;
}
