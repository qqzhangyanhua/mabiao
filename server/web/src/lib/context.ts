import type { ContextItem, ContextManifest } from "../api/types";

/**
 * 条目的证据说法。只有 `injected` 才能说「已注入」；磁盘可能生效、来自缓存、按磁盘重建
 * 都不是会话当时的注入原文，页面不得把它们表述成已注入（ADR 0026）。
 */
export type ContextTier =
  | "injected"
  | "injected_no_content"
  | "reconstructed"
  | "cached"
  | "on_disk"
  | "observed";

export const TIER_LABEL: Record<ContextTier, string> = {
  injected: "已注入（原文）",
  injected_no_content: "已注入（未保留原文）",
  reconstructed: "按当前磁盘重建（非当时注入）",
  cached: "来自缓存，无原文",
  on_disk: "磁盘可能生效（非当时注入）",
  observed: "运行中观察到的痕迹",
};

export const TIER_ORDER: ContextTier[] = [
  "injected",
  "injected_no_content",
  "reconstructed",
  "cached",
  "on_disk",
  "observed",
];

export function tierOf(manifest: ContextManifest, item: ContextItem): ContextTier {
  if (item.layer === "on_disk_possible") return "on_disk";
  if (item.layer === "observed") return "observed";
  // injected 层：清单整体来自缓存或没有注入快照时，条目不是当时的注入原文。
  if (manifest.from_cache) return "cached";
  if (!manifest.has_injected_snapshot) return "reconstructed";
  return item.content === undefined ? "injected_no_content" : "injected";
}

export interface TierGroup {
  tier: ContextTier;
  items: ContextItem[];
}

/** 按证据层级分组，固定顺序，空组不出现。 */
export function groupByTier(manifest: ContextManifest): TierGroup[] {
  const groups = new Map<ContextTier, ContextItem[]>();
  for (const item of manifest.items) {
    const tier = tierOf(manifest, item);
    groups.set(tier, [...(groups.get(tier) ?? []), item]);
  }
  return TIER_ORDER.flatMap((tier) => {
    const items = groups.get(tier);
    return items ? [{ tier, items }] : [];
  });
}
