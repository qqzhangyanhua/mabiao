import type { OfficialQuotaDto } from "../types";
import type { ShareCardKind } from "./sharePreference";

export type ShareQuotaWork = "idle" | "load_cache" | "refresh";

export type ShareQuotaResult =
  | { ok: true; dto: OfficialQuotaDto; nowMs: number }
  | { ok: false; message: string };

export type ShareQuotaSession = {
  cacheStarted: boolean;
  refreshStarted: boolean;
  cacheLoading: boolean;
  dto: OfficialQuotaDto | null;
  nowMs: number;
  error: string | null;
};

export function createShareQuotaSession(nowMs: number, openOnQuota: boolean): ShareQuotaSession {
  return {
    cacheStarted: false,
    refreshStarted: false,
    cacheLoading: openOnQuota,
    dto: null,
    nowMs,
    error: null,
  };
}

export function shareQuotaWork(kind: ShareCardKind, session: ShareQuotaSession): ShareQuotaWork {
  if (kind !== "quota") {
    return "idle";
  }
  if (!session.cacheStarted) {
    return "load_cache";
  }
  if (session.cacheLoading) {
    return "idle";
  }
  if (!session.refreshStarted) {
    return "refresh";
  }
  return "idle";
}

export function markShareQuotaCacheStarted(session: ShareQuotaSession): ShareQuotaSession {
  return { ...session, cacheStarted: true, cacheLoading: true };
}

export function markShareQuotaRefreshStarted(session: ShareQuotaSession): ShareQuotaSession {
  return { ...session, refreshStarted: true };
}

export function applyShareQuotaCache(
  session: ShareQuotaSession,
  result: ShareQuotaResult,
): ShareQuotaSession {
  if (result.ok) {
    return {
      ...session,
      dto: result.dto,
      nowMs: result.nowMs,
      cacheLoading: false,
      error: null,
    };
  }
  return {
    ...session,
    cacheLoading: false,
    error: session.dto ? session.error : result.message,
  };
}

export function shareQuotaRefreshLocked(copying: boolean, kind: ShareCardKind): boolean {
  return copying && kind === "quota";
}

export function applyShareQuotaRefresh(
  session: ShareQuotaSession,
  result: ShareQuotaResult,
  copying: boolean,
): ShareQuotaSession {
  if (copying) {
    return session;
  }
  if (result.ok) {
    return {
      ...session,
      dto: result.dto,
      nowMs: result.nowMs,
      error: null,
    };
  }
  return {
    ...session,
    error: session.dto ? session.error : result.message,
  };
}
