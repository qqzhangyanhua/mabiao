import { describe, expect, it } from "vitest";
import { formatClock, relativeTime } from "./format";
import { officialQuotaAmountLabel, officialQuotaExhaustLabel } from "./officialQuotaDisplay";
import {
  firstEligibleQuotaRow,
  quotaCardEmptyCopy,
  QUOTA_CARD_KICKER,
  toQuotaCardViewModel,
} from "./quotaCard";
import type { OfficialQuotaDto, OfficialQuotaRow, OfficialQuotaWindow } from "../types";

const NOW = Date.parse("2026-09-01T12:18:00.000Z");
const CAPTURED_AT = "2026-09-01T12:10:00.000Z";
const RESETS_AT = "2026-09-01T18:00:00.000Z";
const EXHAUST_AT = "2026-09-01T12:50:00.000Z";

const PLACEHOLDER_RE = /暂无数据|暂无估计|重置时间未知|——|(?<!\S)—(?!\S)/;
const RELATIVE_RE = /刚刚|\d+\s*分钟前|\d+\s*小时前|\d+\s*天前/;

function quotaWindow(
  partial: Partial<OfficialQuotaWindow> & Pick<OfficialQuotaWindow, "kind" | "label">,
): OfficialQuotaWindow {
  return {
    used_percent: null,
    resets_at: null,
    used_amount: null,
    limit_amount: null,
    currency: null,
    exhaust: null,
    ...partial,
  };
}

function quotaRow(partial: Partial<OfficialQuotaRow> = {}): OfficialQuotaRow {
  return {
    provider: "cursor",
    application: "Cursor",
    windows: [],
    freshness: "unavailable",
    captured_at: null,
    error: null,
    todo: null,
    plan: null,
    ...partial,
  };
}

function quotaDto(
  rows: OfficialQuotaRow[],
  extra: Partial<Pick<OfficialQuotaDto, "hidden_providers" | "undetected">> = {},
): OfficialQuotaDto {
  return {
    rows,
    alerts_enabled: true,
    stale_after_minutes: 10,
    undetected: extra.undetected ?? [],
    hidden_providers: extra.hidden_providers ?? [],
  };
}

function mixedWindows(): OfficialQuotaWindow[] {
  return [
    quotaWindow({
      kind: "total",
      label: "总量",
      used_percent: 72,
      used_amount: 19,
      limit_amount: 50,
      currency: "USD",
      resets_at: RESETS_AT,
      exhaust: { kind: "hits", at: EXHAUST_AT },
    }),
    quotaWindow({
      kind: "on_demand",
      label: "按需",
      used_amount: 3.2,
      currency: "USD",
    }),
  ];
}

function officialCursor(overrides: Partial<OfficialQuotaRow> = {}): OfficialQuotaRow {
  return quotaRow({
    freshness: "official",
    captured_at: CAPTURED_AT,
    plan: "Pro",
    windows: mixedWindows(),
    ...overrides,
  });
}

function cardText(card: NonNullable<ReturnType<typeof toQuotaCardViewModel>>): string {
  return [
    card.kicker,
    card.accountLabel,
    card.planLabel ?? "",
    ...card.windows.flatMap((window) => [
      window.label,
      window.percentLabel ?? "",
      window.amountLabel ?? "",
      window.resetLabel ?? "",
      window.exhaustLabel ?? "",
    ]),
    card.capturedAtLabel,
  ].join("\n");
}

describe("toQuotaCardViewModel", () => {
  it("renders an official snapshot with every window and an exhaust estimate", () => {
    const card = toQuotaCardViewModel(officialCursor(), NOW);
    expect(card).not.toBeNull();
    if (!card) {
      return;
    }
    expect(card.kicker).toBe(QUOTA_CARD_KICKER);
    expect(card.accountLabel).toBe("Cursor");
    expect(card.planLabel).toBe("Pro");
    expect(card.windows).toHaveLength(2);
    expect(card.windows[0]).toMatchObject({
      label: "总量",
      percentLabel: "72%",
      amountLabel: officialQuotaAmountLabel({
        used_amount: 19,
        limit_amount: 50,
        currency: "USD",
      }),
      resetLabel: `重置 ${formatClock(RESETS_AT)}`,
      exhaustLabel: officialQuotaExhaustLabel({ kind: "hits", at: EXHAUST_AT }, NOW),
    });
    expect(card.windows[1]).toMatchObject({
      label: "按需",
      percentLabel: null,
      amountLabel: officialQuotaAmountLabel({
        used_amount: 3.2,
        limit_amount: null,
        currency: "USD",
      }),
      resetLabel: null,
      exhaustLabel: null,
    });
    expect(card.capturedAtLabel).toBe(`数据截至 ${formatClock(CAPTURED_AT)}`);
    expect(card.windows[0]?.exhaustLabel).toContain("预计");
    expect(card.windows[0]?.exhaustLabel).toContain("撞线");
  });

  it("keeps percent and amount side by side without converting either", () => {
    const card = toQuotaCardViewModel(officialCursor(), NOW);
    expect(card?.windows[0]?.percentLabel).toBe("72%");
    expect(card?.windows[0]?.amountLabel).toBe("已用 $19.00 / 共 $50.00");
    expect(card?.windows[0]?.percentLabel).not.toBe("38%");
    expect(card?.windows[1]?.percentLabel).toBeNull();
    expect(card?.windows[1]?.amountLabel).toBe("已用 $3.20");
  });

  it("still renders a stale snapshot but omits exhaust", () => {
    const card = toQuotaCardViewModel(officialCursor({ freshness: "stale" }), NOW);
    expect(card).not.toBeNull();
    if (!card) {
      return;
    }
    expect(card.capturedAtLabel).toBe(`数据截至 ${formatClock(CAPTURED_AT)}`);
    expect(card.windows[0]?.percentLabel).toBe("72%");
    expect(card.windows.map((window) => window.exhaustLabel)).toEqual([null, null]);
    expect(cardText(card)).not.toMatch(/预计|已打满|打不满/);
  });

  it("does not render when there is no snapshot", () => {
    expect(toQuotaCardViewModel(quotaRow(), NOW)).toBeNull();
    expect(
      toQuotaCardViewModel(
        quotaRow({
          freshness: "unavailable",
          captured_at: CAPTURED_AT,
          windows: mixedWindows(),
        }),
        NOW,
      ),
    ).toBeNull();
    expect(
      toQuotaCardViewModel(
        quotaRow({
          freshness: "official",
          captured_at: CAPTURED_AT,
          windows: [],
        }),
        NOW,
      ),
    ).toBeNull();
    expect(
      toQuotaCardViewModel(
        quotaRow({
          freshness: "official",
          captured_at: null,
          windows: mixedWindows(),
        }),
        NOW,
      ),
    ).toBeNull();
  });

  it("omits missing plan, reset, and exhaust instead of placeholders", () => {
    const card = toQuotaCardViewModel(
      officialCursor({
        plan: null,
        error: "HTTP 500",
        todo: "未配置密钥",
        windows: [
          quotaWindow({
            kind: "auto",
            label: "Auto",
            used_percent: 0,
          }),
        ],
      }),
      NOW,
    );
    expect(card).not.toBeNull();
    if (!card) {
      return;
    }
    expect(card.planLabel).toBeNull();
    expect(card.windows[0]?.resetLabel).toBeNull();
    expect(card.windows[0]?.exhaustLabel).toBeNull();
    expect(card.windows[0]?.percentLabel).toBe("0%");
    const text = cardText(card);
    expect(text).not.toMatch(PLACEHOLDER_RE);
    expect(text).not.toContain("HTTP 500");
    expect(text).not.toContain("未配置密钥");
    expect(text).not.toContain("auto");
    expect(text).not.toContain("cursor");
  });

  it("prints an absolute snapshot clock, not a relative age", () => {
    const card = toQuotaCardViewModel(officialCursor(), NOW);
    expect(card?.capturedAtLabel).toBe(`数据截至 ${formatClock(CAPTURED_AT)}`);
    expect(card?.capturedAtLabel).not.toContain(relativeTime(CAPTURED_AT, NOW));
    expect(cardText(card!)).not.toMatch(RELATIVE_RE);
    expect(cardText(card!)).not.toMatch(PLACEHOLDER_RE);
  });
});

describe("firstEligibleQuotaRow", () => {
  it("picks the first visible row that can render, skipping hidden and empty ones", () => {
    const hidden = quotaRow({
      provider: "claude",
      application: "Claude Code",
      freshness: "official",
      captured_at: CAPTURED_AT,
      windows: mixedWindows(),
    });
    const empty = quotaRow({
      provider: "codex",
      application: "Codex",
      freshness: "unavailable",
    });
    const first = officialCursor({ provider: "cursor", application: "Cursor" });
    const second = officialCursor({
      provider: "custom:abc",
      application: "家里的中转",
      plan: null,
    });
    const picked = firstEligibleQuotaRow(
      quotaDto([hidden, empty, first, second], { hidden_providers: ["claude"] }),
    );
    expect(picked?.provider).toBe("cursor");
  });

  it("allows an unhidden custom provider with a snapshot", () => {
    const custom = officialCursor({
      provider: "custom:abc",
      application: "家里的中转",
      plan: null,
    });
    expect(firstEligibleQuotaRow(quotaDto([custom]))?.application).toBe("家里的中转");
  });

  it("returns null when no visible row can render", () => {
    expect(firstEligibleQuotaRow(quotaDto([]))).toBeNull();
    expect(
      firstEligibleQuotaRow(quotaDto([officialCursor()], { hidden_providers: ["cursor"] })),
    ).toBeNull();
  });
});

describe("quotaCardEmptyCopy", () => {
  it("reuses the quota empty copy when nothing visible is left", () => {
    expect(quotaCardEmptyCopy(quotaDto([], { undetected: ["claude"] })).title).toBe(
      "暂无已登录的官方额度账号",
    );
    expect(quotaCardEmptyCopy(quotaDto([], { hidden_providers: ["claude"] })).title).toBe(
      "所选账号均已隐藏",
    );
  });

  it("explains visible accounts that still have no snapshot", () => {
    const copy = quotaCardEmptyCopy(quotaDto([quotaRow({ freshness: "unavailable" })]));
    expect(copy.title).toBe("还没有可分享的额度快照");
    expect(copy.hint).toContain("快照");
  });
});
