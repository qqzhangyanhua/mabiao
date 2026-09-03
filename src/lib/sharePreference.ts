import {
  DEFAULT_REPORT_POSTER_STYLE_ID,
  resolveReportPosterStyleId,
  type ReportPosterStyleId,
} from "../report/posterStyleRegistry";

/** 本机 webview 偏好：卡片类型、额度账号与周报海报风格。不进备份。 */
export const SHARE_PREFERENCE_STORAGE_KEY = "mabiao:share-preference";

export type ShareCardKind = "week" | "quota";

export type SharePreference = {
  kind: ShareCardKind;
  quotaProvider: string | null;
  posterStyleId: ReportPosterStyleId;
};

export function defaultSharePreference(): SharePreference {
  return { kind: "week", quotaProvider: null, posterStyleId: DEFAULT_REPORT_POSTER_STYLE_ID };
}

function isShareCardKind(value: unknown): value is ShareCardKind {
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
    if (!isShareCardKind(record.kind)) {
      return defaults;
    }
    const quotaProvider =
      typeof record.quotaProvider === "string" && record.quotaProvider.length > 0
        ? record.quotaProvider
        : null;
    const posterStyleId = resolveReportPosterStyleId(record.posterStyleId);
    return { kind: record.kind, quotaProvider, posterStyleId };
  } catch {
    return defaults;
  }
}

export function serializeSharePreference(preference: SharePreference): string {
  return JSON.stringify({
    kind: preference.kind,
    quotaProvider: preference.quotaProvider,
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
