export function formatTokens(value: number): string {
  const abs = Math.abs(value);
  if (abs >= 1e9) return `${(value / 1e9).toFixed(2)}B`;
  if (abs >= 1e6) return `${(value / 1e6).toFixed(2)}M`;
  if (abs >= 1e3) return `${(value / 1e3).toFixed(1)}K`;
  return String(value);
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
