import { EXTREME_REPORT_CASES } from "../lib/reportExtremeFixtures";
import { toPosterViewModel } from "../lib/reportCopy";
import type { PosterViewModel } from "./posterTypes";

/** 固定假数据：只用来验证截图路线，不是真实消耗记录。 */
export const FAKE_POSTER: PosterViewModel = {
  kicker: "码表 · 周报",
  rangeLabel: "2026年8月24日 – 8月30日",
  totalTokensLabel: "12.4M",
  totalUnit: "本周 token",
  totalCostLabel: "$18.60",
  comments: [
    "你这周烧掉了 12.4M token。",
    "你 43% 的 token 是在凌晨烧的。",
    "最活跃的时段是 22:00 到 02:00。",
  ],
  days: [
    { label: "一", tokens: 1_100_000 },
    { label: "二", tokens: 1_800_000 },
    { label: "三", tokens: 3_200_000 },
    { label: "四", tokens: 1_400_000 },
    { label: "五", tokens: 2_100_000 },
    { label: "六", tokens: 1_600_000 },
    { label: "日", tokens: 1_200_000 },
  ],
  sources: [
    { label: "Claude", pct: 52, color: "#f59e0b" },
    { label: "Codex", pct: 31, color: "#8b6cff" },
    { label: "Grok", pct: 17, color: "#f472b6" },
  ],
  stats: [
    { kind: "busiest_day", label: "最忙的一天", value: "周三" },
    {
      kind: "models",
      label: "模型 Top 3",
      value: "claude-opus-4.1 · gpt-5 · grok-4",
      items: ["claude-opus-4.1", "gpt-5", "grok-4"],
    },
    {
      kind: "top_session",
      label: "最贵的一次",
      value: "$4.20 · 重构鉴权中间件",
      amount: "$4.20",
      project: "重构鉴权中间件",
    },
  ],
};

function requirePoster(id: string, dto: Parameters<typeof toPosterViewModel>[0]): PosterViewModel {
  const poster = toPosterViewModel(dto);
  if (!poster) {
    throw new Error(`extreme case ${id} produced no poster`);
  }
  return poster;
}

/** #158 整图收口：与 reportCopy 组合用例同一份 DTO。 */
export const EXTREME_POSTERS: { id: string; label: string; data: PosterViewModel }[] =
  EXTREME_REPORT_CASES.map((item) => ({
    id: item.id,
    label: item.label,
    data: requirePoster(item.id, item.dto),
  }));
