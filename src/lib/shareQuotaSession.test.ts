import { describe, expect, it } from "vitest";
import { formatClock } from "./format";
import { officialQuotaExhaustLabel } from "./officialQuotaDisplay";
import { toQuotaCardViewModel } from "./quotaCard";
import {
  applyShareQuotaCache,
  applyShareQuotaRefresh,
  createShareQuotaSession,
  markShareQuotaCacheStarted,
  markShareQuotaRefreshStarted,
  shareQuotaRefreshLocked,
  shareQuotaWork,
} from "./shareQuotaSession";
import type { OfficialQuotaDto, OfficialQuotaRow, OfficialQuotaWindow } from "../types";

const NOW = Date.parse("2026-09-01T12:18:00.000Z");
const LATER = Date.parse("2026-09-01T12:20:00.000Z");
const CAPTURED_AT = "2026-09-01T12:10:00.000Z";
const FRESH_CAPTURED_AT = "2026-09-01T12:19:00.000Z";
const EXHAUST_AT = "2026-09-01T12:50:00.000Z";

function quotaWindow(): OfficialQuotaWindow {
  return {
    kind: "total",
    label: "总量",
    used_percent: 72,
    resets_at: null,
    used_amount: null,
    limit_amount: null,
    currency: null,
    exhaust: { kind: "hits", at: EXHAUST_AT },
  };
}

function quotaRow(partial: Partial<OfficialQuotaRow> = {}): OfficialQuotaRow {
  return {
    provider: "cursor",
    application: "Cursor",
    windows: [quotaWindow()],
    freshness: "stale",
    captured_at: CAPTURED_AT,
    error: null,
    todo: null,
    plan: "Pro",
    ...partial,
  };
}

function quotaDto(rows: OfficialQuotaRow[] = [quotaRow()]): OfficialQuotaDto {
  return {
    rows,
    alerts_enabled: true,
    stale_after_minutes: 10,
    undetected: [],
    hidden_providers: [],
  };
}

function firstRow(dto: OfficialQuotaDto | null): OfficialQuotaRow {
  const row = dto?.rows[0];
  if (!row) {
    throw new Error("expected a quota row");
  }
  return row;
}

describe("shareQuotaWork", () => {
  it("loads cache first when switching to quota, including when that is the opening kind", () => {
    const session = createShareQuotaSession(NOW, true);
    expect(shareQuotaWork("quota", session)).toBe("load_cache");
    expect(shareQuotaWork("week", session)).toBe("idle");
  });

  it("refreshes once after cache settles, and does not refresh again when returning from week", () => {
    let session = markShareQuotaCacheStarted(createShareQuotaSession(NOW, true));
    expect(shareQuotaWork("quota", session)).toBe("idle");

    session = applyShareQuotaCache(session, { ok: true, dto: quotaDto(), nowMs: NOW });
    expect(shareQuotaWork("quota", session)).toBe("refresh");

    session = markShareQuotaRefreshStarted(session);
    expect(shareQuotaWork("quota", session)).toBe("idle");
    expect(shareQuotaWork("week", session)).toBe("idle");
    expect(shareQuotaWork("quota", session)).toBe("idle");
  });

  it("still refreshes after a cache failure so the dialog is not stuck on the first invoke", () => {
    let session = markShareQuotaCacheStarted(createShareQuotaSession(NOW, false));
    session = applyShareQuotaCache(session, { ok: false, message: "缓存读失败" });
    expect(shareQuotaWork("quota", session)).toBe("refresh");
  });
});

describe("cached preview and refresh apply", () => {
  it("keeps the cached snapshot visible after cache arrives, even before refresh starts", () => {
    let session = markShareQuotaCacheStarted(createShareQuotaSession(NOW, true));
    expect(session.cacheLoading).toBe(true);
    expect(session.dto).toBeNull();

    const cached = quotaDto([quotaRow({ freshness: "stale" })]);
    session = applyShareQuotaCache(session, { ok: true, dto: cached, nowMs: NOW });
    expect(session.cacheLoading).toBe(false);
    expect(session.dto).toEqual(cached);
    expect(shareQuotaWork("quota", session)).toBe("refresh");
  });

  it("replaces a stale cache with a fresh snapshot when not copying, revealing exhaust", () => {
    let session = applyShareQuotaCache(
      markShareQuotaCacheStarted(createShareQuotaSession(NOW, true)),
      { ok: true, dto: quotaDto([quotaRow({ freshness: "stale" })]), nowMs: NOW },
    );
    const cachedCard = toQuotaCardViewModel(firstRow(session.dto), session.nowMs);
    expect(cachedCard?.windows[0]?.exhaustLabel).toBeNull();
    expect(cachedCard?.capturedAtLabel).toBe(`数据截至 ${formatClock(CAPTURED_AT)}`);

    session = applyShareQuotaRefresh(
      markShareQuotaRefreshStarted(session),
      {
        ok: true,
        dto: quotaDto([
          quotaRow({ freshness: "official", captured_at: FRESH_CAPTURED_AT }),
        ]),
        nowMs: LATER,
      },
      false,
    );
    const freshCard = toQuotaCardViewModel(firstRow(session.dto), session.nowMs);
    expect(freshCard?.windows[0]?.exhaustLabel).toBe(
      officialQuotaExhaustLabel({ kind: "hits", at: EXHAUST_AT }, LATER),
    );
    expect(freshCard?.capturedAtLabel).toBe(`数据截至 ${formatClock(FRESH_CAPTURED_AT)}`);
  });

  it("locks late refresh only while copying the quota card, not the week card", () => {
    expect(shareQuotaRefreshLocked(true, "quota")).toBe(true);
    expect(shareQuotaRefreshLocked(true, "week")).toBe(false);
    expect(shareQuotaRefreshLocked(false, "quota")).toBe(false);
  });

  it("discards a late refresh while copying so the preview stays on the cache", () => {
    const cached = quotaDto([quotaRow({ freshness: "stale" })]);
    let session = applyShareQuotaCache(
      markShareQuotaCacheStarted(createShareQuotaSession(NOW, true)),
      { ok: true, dto: cached, nowMs: NOW },
    );
    session = applyShareQuotaRefresh(
      markShareQuotaRefreshStarted(session),
      {
        ok: true,
        dto: quotaDto([quotaRow({ freshness: "official", captured_at: FRESH_CAPTURED_AT })]),
        nowMs: LATER,
      },
      true,
    );
    expect(session.dto).toEqual(cached);
    expect(session.nowMs).toBe(NOW);
    expect(toQuotaCardViewModel(firstRow(session.dto), session.nowMs)?.windows[0]?.exhaustLabel).toBeNull();
    expect(shareQuotaWork("quota", session)).toBe("idle");
  });
});

describe("refresh failure keeps cache rules", () => {
  it("keeps a cached snapshot and does not surface the refresh error over the poster", () => {
    const cached = quotaDto([quotaRow({ freshness: "stale" })]);
    let session = applyShareQuotaCache(
      markShareQuotaCacheStarted(createShareQuotaSession(NOW, true)),
      { ok: true, dto: cached, nowMs: NOW },
    );
    session = applyShareQuotaRefresh(
      markShareQuotaRefreshStarted(session),
      { ok: false, message: "Codex 超时" },
      false,
    );
    expect(session.dto).toEqual(cached);
    expect(session.error).toBeNull();
    const card = toQuotaCardViewModel(firstRow(session.dto), session.nowMs);
    expect(card).not.toBeNull();
    expect(card?.windows[0]?.exhaustLabel).toBeNull();
    expect(card?.capturedAtLabel).toBe(`数据截至 ${formatClock(CAPTURED_AT)}`);
  });

  it("keeps a failed refresh from painting when copying, including its error", () => {
    const cached = quotaDto();
    let session = applyShareQuotaCache(
      markShareQuotaCacheStarted(createShareQuotaSession(NOW, true)),
      { ok: true, dto: cached, nowMs: NOW },
    );
    session = applyShareQuotaRefresh(
      markShareQuotaRefreshStarted(session),
      { ok: false, message: "断网" },
      true,
    );
    expect(session.dto).toEqual(cached);
    expect(session.error).toBeNull();
  });

  it("cannot render when cache and refresh both miss a snapshot", () => {
    let session = applyShareQuotaCache(
      markShareQuotaCacheStarted(createShareQuotaSession(NOW, true)),
      { ok: false, message: "缓存读失败" },
    );
    expect(session.dto).toBeNull();
    expect(session.error).toBe("缓存读失败");

    session = applyShareQuotaRefresh(
      markShareQuotaRefreshStarted(session),
      { ok: false, message: "刷新失败" },
      false,
    );
    expect(session.dto).toBeNull();
    expect(session.error).toBe("刷新失败");
  });
});
