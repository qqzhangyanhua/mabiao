export function formatTokens(value: number): string {
  const abs = Math.abs(value);
  if (abs >= 1e9) return `${(value / 1e9).toFixed(2)}B`;
  if (abs >= 1e6) return `${(value / 1e6).toFixed(2)}M`;
  if (abs >= 1e3) return `${(value / 1e3).toFixed(1)}K`;
  return String(value);
}

/** 字符数用中文千分位，不复用 token 的 K/M/B。 */
export function formatChars(value: number): string {
  return `${value.toLocaleString("zh-CN")} 字符`;
}

export function formatPercent(value: number): string {
  return `${(value * 100).toFixed(value < 0.1 ? 1 : 0)}%`;
}

/** 相对「最后活动」：今天 / N 天前 / 本地日期。 */
export function formatRelativeDay(value: string | null, now: Date = new Date()): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  const start = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
  const then = Date.UTC(date.getFullYear(), date.getMonth(), date.getDate());
  const days = Math.round((start - then) / 86_400_000);
  if (days <= 0) return "今天";
  if (days === 1) return "昨天";
  if (days < 14) return `${days} 天前`;
  return formatDateTime(value).slice(0, 10);
}

export function formatCost(value: number): string {
  if (value === 0) return "$0";
  return Math.abs(value) >= 1 ? `$${value.toFixed(2)}` : `$${value.toFixed(4)}`;
}

/** 本地时间 `YYYY-MM-DD HH:mm`；空值或解析不了显示「—」。 */
export function formatDateTime(value: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
