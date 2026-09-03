import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState, type RefObject } from "react";
import { humanStatus } from "../lib/format";
import type { ShareCardKind } from "../lib/sharePreference";
import {
  applyShareQuotaCache,
  applyShareQuotaRefresh,
  createShareQuotaSession,
  markShareQuotaCacheStarted,
  markShareQuotaRefreshStarted,
  shareQuotaRefreshLocked,
  shareQuotaWork,
  type ShareQuotaSession,
} from "../lib/shareQuotaSession";
import type { OfficialQuotaDto } from "../types";

export function useShareQuota(
  kind: ShareCardKind,
  copyingRef: RefObject<boolean>,
): ShareQuotaSession {
  const [session, setSession] = useState(() =>
    createShareQuotaSession(Date.now(), kind === "quota"),
  );
  const sessionRef = useRef(session);
  const aliveRef = useRef(true);

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  useEffect(() => {
    const work = shareQuotaWork(kind, sessionRef.current);
    if (work === "load_cache") {
      sessionRef.current = markShareQuotaCacheStarted(sessionRef.current);
      setSession(sessionRef.current);
      void invoke<OfficialQuotaDto>("get_official_quota")
        .then((dto) => {
          if (!aliveRef.current) {
            return;
          }
          sessionRef.current = applyShareQuotaCache(sessionRef.current, {
            ok: true,
            dto,
            nowMs: Date.now(),
          });
          setSession(sessionRef.current);
        })
        .catch((caught: unknown) => {
          if (!aliveRef.current) {
            return;
          }
          sessionRef.current = applyShareQuotaCache(sessionRef.current, {
            ok: false,
            message: humanStatus(caught),
          });
          setSession(sessionRef.current);
        });
      return;
    }
    if (work !== "refresh") {
      return;
    }
    sessionRef.current = markShareQuotaRefreshStarted(sessionRef.current);
    setSession(sessionRef.current);
    void invoke<OfficialQuotaDto>("refresh_official_quota")
      .then((dto) => {
        if (!aliveRef.current) {
          return;
        }
        sessionRef.current = applyShareQuotaRefresh(
          sessionRef.current,
          { ok: true, dto, nowMs: Date.now() },
          shareQuotaRefreshLocked(copyingRef.current, kind),
        );
        setSession(sessionRef.current);
      })
      .catch((caught: unknown) => {
        if (!aliveRef.current) {
          return;
        }
        sessionRef.current = applyShareQuotaRefresh(
          sessionRef.current,
          { ok: false, message: humanStatus(caught) },
          shareQuotaRefreshLocked(copyingRef.current, kind),
        );
        setSession(sessionRef.current);
      });
  }, [copyingRef, kind, session.cacheLoading, session.cacheStarted, session.refreshStarted]);

  return session;
}
