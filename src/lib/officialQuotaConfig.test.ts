import { describe, expect, it } from "vitest";
import {
  DEFAULT_OFFICIAL_QUOTA_CONFIG,
  formatAlertThresholdsInput,
  mergeOfficialQuotaConfig,
  parseAlertThresholdsInput,
} from "./officialQuotaConfig";

describe("mergeOfficialQuotaConfig", () => {
  it("keeps custom thresholds when only hidden providers change", () => {
    const current = {
      ...DEFAULT_OFFICIAL_QUOTA_CONFIG,
      alert_thresholds: [50, 90],
      reset_reminder_hours: 24,
    };
    expect(mergeOfficialQuotaConfig(current, { hidden_providers: ["claude"] })).toEqual({
      ...current,
      hidden_providers: ["claude"],
    });
  });

  it("keeps hidden providers when only alerts_enabled flips", () => {
    const current = {
      ...DEFAULT_OFFICIAL_QUOTA_CONFIG,
      hidden_providers: ["grok"],
      alert_thresholds: [70],
    };
    expect(mergeOfficialQuotaConfig(current, { alerts_enabled: false })).toEqual({
      ...current,
      alerts_enabled: false,
    });
  });
});

describe("parseAlertThresholdsInput", () => {
  it("parses comma-separated percents and falls back to defaults", () => {
    expect(parseAlertThresholdsInput("80, 100")).toEqual([80, 100]);
    expect(parseAlertThresholdsInput("90，50, 50, 0, 101")).toEqual([50, 90]);
    expect(parseAlertThresholdsInput("  ")).toEqual([80, 100]);
  });
});

describe("formatAlertThresholdsInput", () => {
  it("joins with a comma and space", () => {
    expect(formatAlertThresholdsInput([50, 90])).toBe("50, 90");
  });
});
