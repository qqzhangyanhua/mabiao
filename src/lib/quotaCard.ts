import type { OfficialQuotaDto, OfficialQuotaRow, OfficialQuotaWindow } from "../types";
import { formatClock } from "./format";
import {
  officialQuotaAmountLabel,
  officialQuotaEmptyCopy,
  officialQuotaExhaustLabel,
} from "./officialQuotaDisplay";
import { officialQuotaProviderLabel, visibleOfficialQuotaRows } from "./overviewLayout";

export const QUOTA_CARD_KICKER = "码表 · 额度";

export type QuotaCardWindowView = {
  label: string;
  percent: number | null;
  percentLabel: string | null;
  amountLabel: string | null;
  resetLabel: string | null;
  exhaustLabel: string | null;
};

export type QuotaCardViewModel = {
  kicker: string;
  accountLabel: string;
  planLabel: string | null;
  windows: QuotaCardWindowView[];
  capturedAtLabel: string;
};

function absoluteClock(iso: string | null): string | null {
  if (!iso || Number.isNaN(Date.parse(iso))) {
    return null;
  }
  const clock = formatClock(iso);
  return clock === "—" ? null : clock;
}

function percentLabel(value: number | null): string | null {
  if (value == null || Number.isNaN(value)) {
    return null;
  }
  return `${value.toFixed(0)}%`;
}

function canRenderQuotaCard(row: OfficialQuotaRow): boolean {
  return (
    row.freshness !== "unavailable" &&
    row.windows.length > 0 &&
    absoluteClock(row.captured_at) != null
  );
}

function windowView(
  window: OfficialQuotaWindow,
  freshness: OfficialQuotaRow["freshness"],
  nowMs: number,
): QuotaCardWindowView {
  const resetClock = absoluteClock(window.resets_at);
  return {
    label: window.label,
    percent: window.used_percent,
    percentLabel: percentLabel(window.used_percent),
    amountLabel: officialQuotaAmountLabel(window),
    resetLabel: resetClock ? `重置 ${resetClock}` : null,
    exhaustLabel:
      freshness === "official" ? officialQuotaExhaustLabel(window.exhaust, nowMs) : null,
  };
}

export function toQuotaCardViewModel(
  row: OfficialQuotaRow,
  nowMs: number,
): QuotaCardViewModel | null {
  const capturedClock = absoluteClock(row.captured_at);
  if (!canRenderQuotaCard(row) || !capturedClock) {
    return null;
  }
  const plan = row.plan?.trim() || null;
  return {
    kicker: QUOTA_CARD_KICKER,
    accountLabel: row.application.trim() || officialQuotaProviderLabel(row.provider),
    planLabel: plan,
    windows: row.windows.map((window) => windowView(window, row.freshness, nowMs)),
    capturedAtLabel: `数据截至 ${capturedClock}`,
  };
}

export function eligibleQuotaRows(dto: OfficialQuotaDto): OfficialQuotaRow[] {
  return visibleOfficialQuotaRows(dto.rows, dto.hidden_providers).filter(canRenderQuotaCard);
}

export function firstEligibleQuotaRow(dto: OfficialQuotaDto): OfficialQuotaRow | null {
  return eligibleQuotaRows(dto)[0] ?? null;
}

export function resolveQuotaAccount(
  eligible: readonly OfficialQuotaRow[],
  rememberedProvider: string | null | undefined,
): OfficialQuotaRow | null {
  if (rememberedProvider) {
    const remembered = eligible.find((row) => row.provider === rememberedProvider);
    if (remembered) {
      return remembered;
    }
  }
  return eligible[0] ?? null;
}

export function quotaCardEmptyCopy(dto: OfficialQuotaDto): { title: string; hint: string } {
  const visible = visibleOfficialQuotaRows(dto.rows, dto.hidden_providers);
  if (visible.length === 0) {
    return officialQuotaEmptyCopy({ ...dto, rows: [] });
  }
  return {
    title: "还没有可分享的额度快照",
    hint: "可见账号都还没有官方额度快照。取数成功后才能出图。",
  };
}
