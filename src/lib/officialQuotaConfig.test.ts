import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  DEFAULT_OFFICIAL_QUOTA_CONFIG,
  formatAlertThresholdsInput,
  mergeOfficialQuotaConfig,
  parseAlertThresholdsInput,
  persistOfficialQuotaConfig,
} from "./officialQuotaConfig";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

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

describe("persistOfficialQuotaConfig", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("uses a single atomic patch_official_quota_config IPC without reading first", async () => {
    const saved = {
      ...DEFAULT_OFFICIAL_QUOTA_CONFIG,
      alerts_enabled: false,
      hidden_providers: ["claude"],
    };
    vi.mocked(invoke).mockResolvedValueOnce(saved);

    const result = await persistOfficialQuotaConfig({ alerts_enabled: false });
    expect(result).toEqual(saved);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("patch_official_quota_config", {
      patch: { alerts_enabled: false },
    });
  });
});
