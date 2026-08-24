import type { OfficialQuotaRow } from "../types";

export const TRAY_QUOTA_LAYOUT_KEY = "mabiao:tray-quota-layout";
export const TRAY_QUOTA_WIDTH = 372;
export const TRAY_QUOTA_MIN_HEIGHT = 120;
export const TRAY_QUOTA_MAX_HEIGHT = 640;
export const TRAY_QUOTA_DRAG_THRESHOLD = 4;

export type TrayQuotaLayout = {
  order: string[];
  collapsed: string[];
};

export function defaultTrayQuotaLayout(): TrayQuotaLayout {
  return { order: [], collapsed: [] };
}

export function parseTrayQuotaLayout(raw: string | null): TrayQuotaLayout {
  const empty = defaultTrayQuotaLayout();
  if (raw == null || raw === "") {
    return empty;
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return empty;
    }
    const record = parsed as Record<string, unknown>;
    return {
      order: readIdList(record.order),
      collapsed: readIdList(record.collapsed),
    };
  } catch {
    return empty;
  }
}

export function loadTrayQuotaLayout(): TrayQuotaLayout {
  try {
    return parseTrayQuotaLayout(localStorage.getItem(TRAY_QUOTA_LAYOUT_KEY));
  } catch {
    return defaultTrayQuotaLayout();
  }
}

export function saveTrayQuotaLayout(layout: TrayQuotaLayout): void {
  try {
    localStorage.setItem(TRAY_QUOTA_LAYOUT_KEY, JSON.stringify(layout));
  } catch {
    /* quota / private mode */
  }
}

export function ensureTrayQuotaOrder(order: string[], providers: string[]): string[] {
  const allowed = new Set(providers);
  const seen = new Set<string>();
  const next: string[] = [];
  for (const id of order) {
    if (!allowed.has(id) || seen.has(id)) {
      continue;
    }
    next.push(id);
    seen.add(id);
  }
  for (const id of providers) {
    if (seen.has(id)) {
      continue;
    }
    next.push(id);
    seen.add(id);
  }
  return next;
}

export function sortQuotaRows<T extends { provider: string }>(rows: T[], order: string[]): T[] {
  const ranked = ensureTrayQuotaOrder(
    order,
    rows.map((row) => row.provider),
  );
  const rank = new Map(ranked.map((id, index) => [id, index]));
  return rows
    .slice()
    .sort((left, right) => (rank.get(left.provider) ?? 0) - (rank.get(right.provider) ?? 0));
}

export function moveTrayQuotaProvider(order: string[], from: string, to: string): string[] {
  if (from === to) {
    return order;
  }
  const fromIndex = order.indexOf(from);
  const toIndex = order.indexOf(to);
  if (fromIndex < 0 || toIndex < 0) {
    return order;
  }
  const next = order.filter((id) => id !== from);
  const insertAt = fromIndex < toIndex ? next.indexOf(to) + 1 : next.indexOf(to);
  next.splice(insertAt, 0, from);
  return next;
}

export function persistTrayQuotaMove(
  saved: string[],
  visible: string[],
  from: string,
  to: string,
): string[] {
  const visibleOrder = moveTrayQuotaProvider(ensureTrayQuotaOrder(saved, visible), from, to);
  const leftovers = saved.filter((id) => !visible.includes(id));
  return uniqueIds([...visibleOrder, ...leftovers]);
}

export function toggleTrayQuotaCollapsed(collapsed: string[], provider: string): string[] {
  return collapsed.includes(provider)
    ? collapsed.filter((id) => id !== provider)
    : [...collapsed, provider];
}

export function isTrayQuotaCollapsed(collapsed: string[], provider: string): boolean {
  return collapsed.includes(provider);
}

export function trayQuotaRowSummary(
  row: Pick<OfficialQuotaRow, "windows" | "error"> & { todo?: string | null },
): string {
  if (row.windows.length === 0) {
    return shorten(row.todo ?? row.error ?? "暂无");
  }
  let max: number | null = null;
  for (const window of row.windows) {
    if (window.used_percent != null && (max == null || window.used_percent > max)) {
      max = window.used_percent;
    }
  }
  const count = `${row.windows.length} 窗`;
  return max == null ? count : `${Math.round(max)}% · ${count}`;
}

export function clampTrayQuotaWindowHeight(contentHeight: number): number {
  return Math.min(TRAY_QUOTA_MAX_HEIGHT, Math.max(TRAY_QUOTA_MIN_HEIGHT, Math.round(contentHeight)));
}

export function quotaProviderFromPoint(x: number, y: number): string | null {
  if (typeof document === "undefined") {
    return null;
  }
  const node = document.elementFromPoint(x, y);
  if (!(node instanceof Element)) {
    return null;
  }
  return node.closest("[data-quota-provider]")?.getAttribute("data-quota-provider") || null;
}

function readIdList(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return uniqueIds(value.filter((id): id is string => typeof id === "string" && id.length > 0));
}

function uniqueIds(ids: string[]): string[] {
  const seen = new Set<string>();
  const next: string[] = [];
  for (const id of ids) {
    if (seen.has(id)) {
      continue;
    }
    seen.add(id);
    next.push(id);
  }
  return next;
}

function shorten(text: string): string {
  return text.length > 18 ? `${text.slice(0, 17)}…` : text;
}
