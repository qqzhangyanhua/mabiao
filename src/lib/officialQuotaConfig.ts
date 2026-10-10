import { invoke } from "@tauri-apps/api/core";
import type { OfficialQuotaConfig } from "../types";

export const DEFAULT_OFFICIAL_QUOTA_CONFIG: OfficialQuotaConfig = {
  alerts_enabled: true,
  hidden_providers: [],
  alert_thresholds: [80, 100],
  reset_reminder_hours: 12,
  reset_reminder_max_used_percent: 50,
};

export function mergeOfficialQuotaConfig(
  current: OfficialQuotaConfig,
  patch: Partial<OfficialQuotaConfig>,
): OfficialQuotaConfig {
  return {
    alerts_enabled: patch.alerts_enabled ?? current.alerts_enabled,
    hidden_providers: patch.hidden_providers ?? current.hidden_providers,
    alert_thresholds: patch.alert_thresholds ?? current.alert_thresholds,
    reset_reminder_hours: patch.reset_reminder_hours ?? current.reset_reminder_hours,
    reset_reminder_max_used_percent:
      patch.reset_reminder_max_used_percent ?? current.reset_reminder_max_used_percent,
  };
}

/** 设置页输入：逗号 / 空白分隔，1–100、去重升序。空则回落默认。 */
export function parseAlertThresholdsInput(raw: string): number[] {
  const values = raw
    .split(/[,，\s]+/)
    .map((part) => Number.parseInt(part, 10))
    .filter((value) => Number.isInteger(value) && value >= 1 && value <= 100);
  const unique = [...new Set(values)].sort((a, b) => a - b);
  return unique.length > 0 ? unique : [...DEFAULT_OFFICIAL_QUOTA_CONFIG.alert_thresholds];
}

export function formatAlertThresholdsInput(values: number[]): string {
  return values.join(", ");
}

export async function persistOfficialQuotaConfig(
  patch: Partial<OfficialQuotaConfig>,
): Promise<OfficialQuotaConfig> {
  const current = await invoke<OfficialQuotaConfig>("get_official_quota_config");
  const next = mergeOfficialQuotaConfig(current, patch);
  await invoke("save_official_quota_config", { config: next });
  return next;
}
