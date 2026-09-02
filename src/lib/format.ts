import type { Filter } from "../types";

export function humanStatus(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  if (/ipc|webview|transformCallback|not allowed|unavailable|Cannot read/i.test(text)) {
    return "IPC 未连通";
  }
  return text;
}

export function formatTokens(n: number): string {
  return n.toLocaleString("zh-CN");
}

export function formatCompact(n: number): string {
  const abs = Math.abs(n);
  if (abs >= 1_000_000_000) {
    return `${trimNum(n / 1_000_000_000)}B`;
  }
  if (abs >= 1_000_000) {
    return `${trimNum(n / 1_000_000)}M`;
  }
  if (abs >= 10_000) {
    return `${trimNum(n / 1_000)}K`;
  }
  return n.toLocaleString("zh-CN");
}

function trimNum(n: number): string {
  return n
    .toFixed(2)
    .replace(/\.00$/, "")
    .replace(/(\.\d)0$/, "$1");
}

export function formatUsd(n: number | null, unpriced: boolean): string {
  if (n == null) {
    return unpriced ? "—" : "$0.00";
  }
  return `$${n.toFixed(2)}`;
}

/** 金额本身：满 1 分用两位，否则四位。不处理 null / 未配置。 */
export function formatUsdAmount(n: number): string {
  if (Math.abs(n) >= 0.01) {
    return `$${n.toFixed(2)}`;
  }
  return `$${n.toFixed(4)}`;
}

export function formatCost(n: number | null, unpriced: boolean): string {
  if (unpriced && n == null) {
    return "—";
  }
  if (n == null) {
    return "0";
  }
  return n.toFixed(4);
}

export function deltaPct(current: number, previous: number | null): number | null {
  if (previous == null) {
    return null;
  }
  if (previous === 0) {
    return current === 0 ? 0 : null;
  }
  return ((current - previous) / previous) * 100;
}

export function formatDelta(
  pct: number | null,
  vsLabel = "上期",
): { text: string; tone: "up" | "down" | "flat" } | null {
  if (pct == null) {
    return null;
  }
  if (Math.abs(pct) < 0.05) {
    return { text: `持平 vs ${vsLabel}`, tone: "flat" };
  }
  const arrow = pct > 0 ? "↑" : "↓";
  return {
    text: `${arrow} ${Math.abs(pct).toFixed(1)}% vs ${vsLabel}`,
    tone: pct > 0 ? "up" : "down",
  };
}

const sourceNames: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  factory: "Droid",
  droid: "Droid",
  pi: "Pi",
  omp: "OMP",
  opencode: "OpenCode",
  kimi: "Kimi CLI",
  dsh: "DeepSeek Harness",
  gemini: "Gemini CLI",
  grok: "Grok CLI",
  qwen: "Qwen Code",
  cursor: "Cursor",
  cursor_agent: "Cursor Agent",
  copilot: "GitHub Copilot CLI",
  hermes: "Hermes",
  antigravity: "Antigravity",
  devin: "Devin",
};

export function weeklyCountLabel(source: string, count: number): string {
  if (source === "cursor") {
    return `共 ${count} 条事件`;
  }
  return `共 ${count} 个会话`;
}

export function sourceLabel(source: string): string {
  return sourceNames[source] ?? source;
}

export function sourceFilterOptions(usageSources: string[]): string[] {
  const sources = new Set(usageSources);
  sources.add("cursor");
  return [...sources].sort();
}

export function projectLabel(path: string): string {
  if (!path) {
    return "未标注";
  }
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function shortId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 8)}…` : id;
}

export function relativeTime(iso: string, now = Date.now()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) {
    return iso;
  }
  const mins = Math.max(0, Math.floor((now - t) / 60000));
  if (mins < 1) {
    return "刚刚";
  }
  if (mins < 60) {
    return `${mins} 分钟前`;
  }
  const hours = Math.floor(mins / 60);
  if (hours < 24) {
    return `${hours} 小时前`;
  }
  return `${Math.floor(hours / 24)} 天前`;
}

export function formatWindowClock(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) {
    return iso;
  }
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function formatHoursMinutes(totalMinutes: number): string {
  const mins = Math.max(0, Math.round(totalMinutes));
  const hours = Math.floor(mins / 60);
  const minutes = mins % 60;
  if (hours <= 0) {
    return `${minutes}m`;
  }
  return `${hours}h ${minutes}m`;
}

export function formatDuration(from: string | null, to: string | null): string | null {
  if (!from || !to) {
    return null;
  }
  const start = Date.parse(from);
  const end = Date.parse(to);
  if (Number.isNaN(start) || Number.isNaN(end) || end <= start) {
    return null;
  }
  return formatHoursMinutes((end - start) / 60_000);
}

export function formatRatio(value: number | null, digits = 1): string {
  if (value == null || Number.isNaN(value)) {
    return "—";
  }
  return value.toFixed(digits);
}

/** 与使用统计页 `cache_hit_rate` 同口径：cache_read / (input + cache_read)。分母为 0 时为 null。 */
export function cacheHitRate(cacheReadTokens: number, inputTokens: number): number | null {
  const denominator = inputTokens + cacheReadTokens;
  if (denominator <= 0) {
    return null;
  }
  return cacheReadTokens / denominator;
}

export function formatPercent(value: number | null): string {
  if (value == null || Number.isNaN(value)) {
    return "—";
  }
  return `${(value * 100).toFixed(1)}%`;
}

/** 来源页命中率：没有缓存口径时显示「无法计算」，不要 0%。 */
export function formatCacheHitRate(value: number | null): string {
  if (value == null || Number.isNaN(value)) {
    return "无法计算";
  }
  return `${(value * 100).toFixed(1)}%`;
}

export function formatBytes(n: number): string {
  return `${n.toLocaleString("zh-CN")} B`;
}

export function formatClock(iso: string | null): string {
  if (!iso) {
    return "—";
  }
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) {
    return iso;
  }
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function formatRangeLabel(filter: Filter, preset: string): string {
  if (preset === "today") {
    return "今天";
  }
  if (preset === "month") {
    return "本月";
  }
  if (preset === "all" || !filter.from || !filter.to) {
    return "全部历史";
  }
  return `${filter.from.slice(0, 10)} ~ ${filter.to.slice(0, 10)}`;
}

export function providerChannel(name: string): string {
  const official = [
    "official",
    "anthropic",
    "openai",
    "google",
    "gemini",
    "xai",
    "grok",
    "codex_local_access",
    "deepseek-official",
  ];
  if (!name || name === "（未标注）") {
    return "未标注";
  }
  return official.includes(name) ? "官方" : "中转";
}

export type CallRangePreset = "today" | "3" | "7" | "custom";

export function rangeFromPreset(preset: string): { from: string | null; to: string | null } {
  if (/^\d+$/.test(preset)) {
    const days = Number(preset);
    if (days > 0) {
      const to = new Date();
      const from = new Date(to.getTime() - days * 24 * 3600 * 1000);
      return { from: from.toISOString(), to: to.toISOString() };
    }
  }
  if (preset === "today") {
    const now = new Date();
    const from = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    return { from: from.toISOString(), to: now.toISOString() };
  }
  if (preset === "month") {
    const now = new Date();
    const from = new Date(now.getFullYear(), now.getMonth(), 1);
    return { from: from.toISOString(), to: now.toISOString() };
  }
  return { from: null, to: null };
}

/** 把两个 `yyyy-mm-dd` 日期输入换算成覆盖全天的 ISO 起止时间。 */
export function customRangeFilter(
  from: string,
  to: string,
): { from: string | null; to: string | null } {
  const fromDate = new Date(`${from}T00:00:00`);
  const toDate = new Date(`${to}T23:59:59.999`);
  if (Number.isNaN(fromDate.getTime()) || Number.isNaN(toDate.getTime())) {
    return { from: null, to: null };
  }
  return { from: fromDate.toISOString(), to: toDate.toISOString() };
}

export function callRangeWindow(
  preset: CallRangePreset,
  customFrom: string,
  customTo: string,
): { from: string | null; to: string | null } {
  if (preset !== "custom") {
    return rangeFromPreset(preset);
  }
  if (!customFrom || !customTo) {
    return rangeFromPreset("7");
  }
  const range = customRangeFilter(customFrom, customTo);
  return range.from && range.to ? range : rangeFromPreset("7");
}

export function filterWithCallRange(
  filter: Filter,
  preset: CallRangePreset,
  customFrom: string,
  customTo: string,
): Filter {
  const range = callRangeWindow(preset, customFrom, customTo);
  return { ...filter, from: range.from, to: range.to };
}

export function previousFilter(filter: Filter, preset: string): Filter | null {
  if (
    preset !== "3" &&
    preset !== "7" &&
    preset !== "30" &&
    preset !== "custom" &&
    preset !== "today" &&
    preset !== "month"
  ) {
    return null;
  }
  if (!filter.from || !filter.to) {
    return null;
  }
  const from = Date.parse(filter.from);
  const to = Date.parse(filter.to);
  if (Number.isNaN(from) || Number.isNaN(to) || to <= from) {
    return null;
  }
  return {
    ...filter,
    from: new Date(from - (to - from)).toISOString(),
    to: filter.from,
  };
}
