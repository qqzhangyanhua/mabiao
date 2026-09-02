export const LOW_CACHE_HIT_LIMIT = 20;
export const CURSOR_ACCOUNT_SOURCE = "cursor";

export function lowCacheHitEmptyTitle(source: string | null, computable: boolean): string {
  if (!source) {
    return "点上来源行，查看该来源命中率最低的会话";
  }
  if (!computable) {
    return "无法计算";
  }
  return "暂无可以计算命中率的会话";
}

export function lowCacheHitEmptyHint(
  source: string | null,
  computable: boolean,
): string | undefined {
  if (!source) {
    return "只在同一来源内比较。没有缓存口径的来源会显示无法计算，不会记成 0%。";
  }
  if (source === CURSOR_ACCOUNT_SOURCE) {
    return "Cursor 账号用量不是本机会话，不能下钻到对话记录。";
  }
  if (!computable) {
    return "该来源当前筛选范围内没有缓存读或缓存写，无法计算命中率。";
  }
  return undefined;
}
