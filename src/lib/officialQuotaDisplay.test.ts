import { describe, expect, it } from "vitest";
import type { OfficialQuotaDto } from "../types";
import {
  OFFICIAL_QUOTA_FRESHNESS_STATUS,
  officialQuotaAgeLabel,
  officialQuotaEmptyCopy,
  officialQuotaFreshnessTitle,
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
