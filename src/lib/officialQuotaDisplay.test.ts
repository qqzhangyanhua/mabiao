import { describe, expect, it } from "vitest";
import type { OfficialQuotaDto } from "../types";
import {
  OFFICIAL_QUOTA_FRESHNESS_STATUS,
  formatQuotaAmount,
  officialQuotaAgeLabel,
  officialQuotaAmountLabel,
  officialQuotaEmptyCopy,
  officialQuotaFreshnessTitle,
  officialQuotaNotice,
  officialQuotaRefreshHint,
  officialQuotaSettingsRefreshNote,
  officialQuotaUndetectedNote,
} from "./officialQuotaDisplay";

const now = Date.parse("2026-08-23T07:20:00.000Z");

describe("officialQuotaAgeLabel", () => {
  it("returns null when capture time is missing or invalid", () => {
    expect(officialQuotaAgeLabel(null, now)).toBeNull();
    expect(officialQuotaAgeLabel("not-a-date", now)).toBeNull();
  });

  it("uses relative time from the capture stamp", () => {
    expect(officialQuotaAgeLabel("2026-08-23T07:17:00.000Z", now)).toBe("3 分钟前");
    expect(officialQuotaAgeLabel("2026-08-23T07:20:20.000Z", now)).toBe("刚刚");
  });
});

describe("officialQuotaFreshnessTitle", () => {
  it("explains official as a fresh snapshot, not a quota reset", () => {
    expect(
      officialQuotaFreshnessTitle("official", "2026-08-23T07:17:00.000Z", 10),
    ).toContain("10 分钟内视为新鲜");
  });

  it("explains stale as a cache timeout", () => {
    const title = officialQuotaFreshnessTitle("stale", "2026-08-23T07:00:00.000Z", 10);
    expect(title).toContain("已过期指缓存超时，不是额度用完");
    expect(title).toContain("10 分钟");
  });

  it("keeps unavailable without a capture clock", () => {
    expect(officialQuotaFreshnessTitle("unavailable", null, 10)).toBe("尚未取到官方额度");
  });
});

describe("officialQuotaRefreshHint", () => {
  it("states the refresh interval and stale threshold together", () => {
    expect(officialQuotaRefreshHint(10)).toBe(
      "每 10 分钟自动刷新 · 超过 10 分钟未更新标为过期（指缓存）",
    );
  });
});

describe("officialQuotaSettingsRefreshNote", () => {
  it("says when the snapshot is taken and what stale means", () => {
    expect(officialQuotaSettingsRefreshNote(10)).toContain("每 10 分钟自动再刷");
    expect(officialQuotaSettingsRefreshNote(10)).toContain("仍显示上次数字");
  });
});

describe("officialQuotaUndetectedNote", () => {
  it("names missing accounts with the same labels as 配置显示", () => {
    expect(officialQuotaUndetectedNote([])).toBeNull();
    expect(officialQuotaUndetectedNote(["claude", "codex"])).toBe(
      "未检测到本机登录态、暂不显示：Claude Code、Codex。登录对应客户端后会自动出现。",
    );
  });
});

describe("officialQuotaEmptyCopy", () => {
  it("explains loading, undetected accounts, and all-hidden separately", () => {
    expect(officialQuotaEmptyCopy(null).title).toBe("正在读取官方额度…");
    const undetected = officialQuotaEmptyCopy({
      rows: [],
      alerts_enabled: true,
      stale_after_minutes: 10,
      undetected: ["claude"],
      hidden_providers: [],
    } satisfies OfficialQuotaDto);
    expect(undetected.title).toBe("暂无已登录的官方额度账号");
    expect(undetected.hint).toContain("Claude Code");
    const hidden = officialQuotaEmptyCopy({
      rows: [],
      alerts_enabled: true,
      stale_after_minutes: 10,
      undetected: [],
      hidden_providers: ["claude"],
    } satisfies OfficialQuotaDto);
    expect(hidden.title).toBe("所选账号均已隐藏");
  });
});

describe("OFFICIAL_QUOTA_FRESHNESS_STATUS", () => {
  it("keeps the three visible labels", () => {
    expect(OFFICIAL_QUOTA_FRESHNESS_STATUS).toEqual({
      official: "官方",
      stale: "已过期",
      unavailable: "暂无",
    });
  });
});

describe("formatQuotaAmount", () => {
  it("uses the currency symbol when it knows one", () => {
    expect(formatQuotaAmount(19, "USD")).toBe("$19.00");
    expect(formatQuotaAmount(50, "cny")).toBe("¥50.00");
  });

  it("falls back to a trailing code for currencies without a symbol", () => {
    expect(formatQuotaAmount(19, "SGD")).toBe("19.00 SGD");
  });

  it("drops the money marker entirely when the currency is missing", () => {
    expect(formatQuotaAmount(19, null)).toBe("19.00");
  });

  it("drops the cents once the number is large enough not to need them", () => {
    expect(formatQuotaAmount(1234.56, "USD")).toBe("$1,235");
  });
});

describe("officialQuotaAmountLabel", () => {
  it("shows used against the limit when both are known", () => {
    expect(officialQuotaAmountLabel({ used_amount: 19, limit_amount: 50, currency: "USD" })).toBe(
      "已用 $19.00 / 共 $50.00",
    );
  });

  it("degrades to used-only when there is no limit", () => {
    expect(officialQuotaAmountLabel({ used_amount: 19, limit_amount: null, currency: "USD" })).toBe(
      "已用 $19.00",
    );
  });

  it("degrades to limit-only when only the cap came back", () => {
    expect(officialQuotaAmountLabel({ used_amount: null, limit_amount: 50, currency: "USD" })).toBe(
      "共 $50.00",
    );
  });

  it("degrades to bare numbers when the currency is missing", () => {
    expect(officialQuotaAmountLabel({ used_amount: 19, limit_amount: 50, currency: null })).toBe(
      "已用 19.00 / 共 50.00",
    );
  });

  it("returns null when there is no money to show, so the row skips that line", () => {
    expect(
      officialQuotaAmountLabel({ used_amount: null, limit_amount: null, currency: "USD" }),
    ).toBeNull();
  });
});

describe("officialQuotaNotice", () => {
  it("treats a missing secret as a todo, not a fetch error", () => {
    expect(
      officialQuotaNotice({
        todo: "未配置密钥，请在设置页重新填写",
        error: null,
      }),
    ).toEqual({ kind: "todo", text: "未配置密钥，请在设置页重新填写" });
  });

  it("keeps a real fetch failure as an error", () => {
    expect(
      officialQuotaNotice({
        todo: null,
        error: "密钥无效或已失效，请在设置页更新密钥",
      }),
    ).toEqual({ kind: "error", text: "密钥无效或已失效，请在设置页更新密钥" });
  });

  it("prefers the todo when both are present", () => {
    expect(
      officialQuotaNotice({
        todo: "未配置密钥，请在设置页重新填写",
        error: "网络不通，连不上这个地址",
      }),
    ).toEqual({ kind: "todo", text: "未配置密钥，请在设置页重新填写" });
  });

  it("returns null when there is nothing to say", () => {
    expect(officialQuotaNotice({ todo: null, error: null })).toBeNull();
  });
});
