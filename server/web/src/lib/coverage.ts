import type { CoverageView } from "../api/types";

/** 成员最近一天的数据落后多少天算「有缺口」。 */
export const GAP_DAYS = 3;

const DAY_MS = 86_400_000;

export type CoverageStatus = "never" | "stale" | "ok";

/**
 * `covered_through` 是 UTC 日期。只要差几天的量级，用 UTC 日历比较，不引入时区换算。
 * 成员停用后不再推送，缺口是预期的，调用方对停用账号不提示。
 */
export function coverageStatus(
  row: Pick<CoverageView, "covered_through">,
  now: Date,
): CoverageStatus {
  if (!row.covered_through) return "never";
  const through = Date.parse(`${row.covered_through}T00:00:00Z`);
  if (Number.isNaN(through)) return "never";
  const today = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
  return (today - through) / DAY_MS > GAP_DAYS ? "stale" : "ok";
}
