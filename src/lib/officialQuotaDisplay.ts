import type {
  OfficialQuotaDto,
  OfficialQuotaFreshness,
  OfficialQuotaWindow,
} from "../types";
import { formatClock, relativeTime } from "./format";
import { officialQuotaProviderLabel } from "./overviewLayout";

export const OFFICIAL_QUOTA_FRESHNESS_STATUS: Record<OfficialQuotaFreshness, string> = {
  official: "官方",
  stale: "已过期",
  unavailable: "暂无",
};

/** 认得出符号的币种直接写符号，其余用代码后缀，缺币种就只给数字。 */
const CURRENCY_SYMBOLS: Record<string, string> = {
  USD: "$",
  CNY: "¥",
  EUR: "€",
  JPY: "¥",
  GBP: "£",
};

export function formatQuotaAmount(value: number, currency: string | null): string {
  // 小数位跟着量级走：$0.42 要看得见分，$1234 不需要。
  const digits = Math.abs(value) < 100 ? 2 : 0;
  const text = value.toLocaleString("en-US", {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
  if (!currency) {
    return text;
  }
  const symbol = CURRENCY_SYMBOLS[currency.toUpperCase()];
  return symbol ? `${symbol}${text}` : `${text} ${currency.toUpperCase()}`;
}

/**
 * 进度条旁边那行金额：`已用 $19 / 共 $50`。
 *
 * 缺上限时降级成只报已用——充值制的站点常常只认已用，那一行仍然有用。
 * 两个都没有返回 null，界面就不画这一行。
 */
export function officialQuotaAmountLabel(
  window: Pick<OfficialQuotaWindow, "used_amount" | "limit_amount" | "currency">,
): string | null {
  const used = window.used_amount;
  const limit = window.limit_amount;
  const currency = window.currency;
  if (used != null && limit != null) {
    return `已用 ${formatQuotaAmount(used, currency)} / 共 ${formatQuotaAmount(limit, currency)}`;
  }
  if (used != null) {
    return `已用 ${formatQuotaAmount(used, currency)}`;
  }
  if (limit != null) {
    return `共 ${formatQuotaAmount(limit, currency)}`;
  }
  return null;
}

export function officialQuotaAgeLabel(
  capturedAt: string | null,
  now = Date.now(),
): string | null {
  if (!capturedAt) {
    return null;
  }
  if (Number.isNaN(Date.parse(capturedAt))) {
    return null;
  }
  return relativeTime(capturedAt, now);
}

export function officialQuotaFreshnessTitle(
  freshness: OfficialQuotaFreshness,
  capturedAt: string | null,
  staleAfterMinutes: number,
): string {
  const clock = capturedAt ? formatClock(capturedAt) : null;
  if (freshness === "unavailable") {
    return "尚未取到官方额度";
  }
  if (freshness === "official") {
    return clock
      ? `官方快照，${clock} 更新。${staleAfterMinutes} 分钟内视为新鲜`
      : `官方快照，${staleAfterMinutes} 分钟内视为新鲜`;
  }
  return clock
    ? `本地缓存超过 ${staleAfterMinutes} 分钟未更新（${clock}）。已过期指缓存超时，不是额度用完`
    : `本地缓存超过 ${staleAfterMinutes} 分钟未更新。已过期指缓存超时，不是额度用完`;
}

export function officialQuotaRefreshHint(staleAfterMinutes: number): string {
  return `每 ${staleAfterMinutes} 分钟自动刷新 · 超过 ${staleAfterMinutes} 分钟未更新标为过期（指缓存）`;
}

export function officialQuotaSettingsRefreshNote(staleAfterMinutes: number): string {
  return `打开总览或点「刷新额度」时取数；总览打开期间每 ${staleAfterMinutes} 分钟自动再刷。超过 ${staleAfterMinutes} 分钟未更新标为过期，仍显示上次数字。`;
}

export function officialQuotaUndetectedNote(undetected: string[]): string | null {
  if (undetected.length === 0) {
    return null;
  }
  const names = undetected.map((id) => officialQuotaProviderLabel(id)).join("、");
  return `未检测到本机登录态、暂不显示：${names}。登录对应客户端后会自动出现。`;
}

export function officialQuotaEmptyCopy(data: OfficialQuotaDto | null): {
  title: string;
  hint: string;
} {
  if (!data) {
    return { title: "正在读取官方额度…", hint: "先显示上次缓存，再后台刷新" };
  }
  const undetected = officialQuotaUndetectedNote(data.undetected);
  if (undetected) {
    return { title: "暂无已登录的官方额度账号", hint: undetected };
  }
  return { title: "所选账号均已隐藏", hint: "在「配置显示」里打开要看的账号" };
}

export type OfficialQuotaPlanKind = "flagship" | "bronze" | "paid" | "free";

/** Ultra / SuperGrok Heavy 金色，Pro 青铜，其余付费铂金，Free 压暗。 */
export function officialQuotaPlanKind(plan: string): OfficialQuotaPlanKind {
  const key = plan.trim().toLowerCase();
  if (key === "ultra" || key === "supergrok heavy") {
    return "flagship";
  }
  if (key === "pro") {
    return "bronze";
  }
  if (key === "free" || key === "x basic") {
    return "free";
  }
  return "paid";
}

export function officialQuotaPlanClass(plan: string): string {
  return `official-quota-plan is-${officialQuotaPlanKind(plan)}`;
}

/** 首页额度行底下那句：待办不是取数失败，两套样式靠 `kind` 分开。 */
export function officialQuotaNotice(row: {
  todo: string | null;
  error: string | null;
}): { kind: "todo" | "error"; text: string } | null {
  if (row.todo) {
    return { kind: "todo", text: row.todo };
  }
  if (row.error) {
    return { kind: "error", text: row.error };
  }
  return null;
}
