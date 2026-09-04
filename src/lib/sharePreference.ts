import {
  DEFAULT_REPORT_POSTER_STYLE_ID,
  resolveReportPosterStyleId,
  type ReportPosterStyleId,
} from "../report/posterStyleRegistry";

/** 本机 webview 偏好：周报海报风格。不进备份。旧字段 kind / quotaProvider 仍可读，额度卡已从分享入口移除。 */
export const SHARE_PREFERENCE_STORAGE_KEY = "mabiao:share-preference";

export type SharePreference = {
  posterStyleId: ReportPosterStyleId;
};

export function defaultSharePreference(): SharePreference {
  return { posterStyleId: DEFAULT_REPORT_POSTER_STYLE_ID };
}

function isLegacyShareKind(value: unknown): boolean {
  return value === "week" || value === "quota";
}

export function parseSharePreference(raw: string | null): SharePreference {
  const defaults = defaultSharePreference();
  if (raw == null || raw === "") {
    return defaults;
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return defaults;
    }
    const record = parsed as Record<string, unknown>;
    if ("kind" in record && !isLegacyShareKind(record.kind)) {
      return defaults;
    }
    return { posterStyleId: resolveReportPosterStyleId(record.posterStyleId) };
  } catch {
    return defaults;
  }
}

export function serializeSharePreference(preference: SharePreference): string {
  return JSON.stringify({
    kind: "week",
    quotaProvider: null,
    posterStyleId: preference.posterStyleId,
  });
}

export function loadSharePreference(): SharePreference {
  try {
    return parseSharePreference(localStorage.getItem(SHARE_PREFERENCE_STORAGE_KEY));
  } catch {
    return defaultSharePreference();
  }
}

export function saveSharePreference(preference: SharePreference): void {
  try {
    localStorage.setItem(SHARE_PREFERENCE_STORAGE_KEY, serializeSharePreference(preference));
  } catch {
    /* quota / private mode */
  }
}
