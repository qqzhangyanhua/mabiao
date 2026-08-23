import { LogicalSize } from "@tauri-apps/api/dpi";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { EmptyState } from "./components/EmptyState";
import { OfficialQuotaList } from "./components/OfficialQuotaPanel";
import { useTheme } from "./hooks/useTheme";
import { visibleOfficialQuotaRows } from "./lib/overviewLayout";
import {
  clampTrayQuotaWindowHeight,
  TRAY_QUOTA_WIDTH,
} from "./lib/trayQuotaLayout";
import type { OfficialQuotaDto } from "./types";

export default function TrayQuotaApp() {
  useTheme();
  const [quota, setQuota] = useState<OfficialQuotaDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [arrangeTick, setArrangeTick] = useState(0);
  const panelRef = useRef<HTMLElement>(null);

  useEffect(() => {
    document.documentElement.classList.add("tray-popup");

    async function load() {
      try {
        setQuota(await invoke<OfficialQuotaDto>("get_official_quota"));
        setError(null);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    }

    void load();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<OfficialQuotaDto>("tray-quota-shown", (event) => {
      setQuota(event.payload);
      setError(null);
    }).then((fn) => {
      if (disposed) {
        fn();
        return;
      }
      unlisten = fn;
    });

    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") {
        void getCurrentWebviewWindow().hide();
      }
    }
    window.addEventListener("keydown", onKey);

    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("keydown", onKey);
      document.documentElement.classList.remove("tray-popup");
    };
  }, []);

  const rows = useMemo(
    () => (quota ? visibleOfficialQuotaRows(quota.rows, quota.hidden_providers) : []),
    [quota],
  );

  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (!panel) {
      return;
    }
    const height = clampTrayQuotaWindowHeight(panel.scrollHeight + 16);
    void getCurrentWebviewWindow()
      .setSize(new LogicalSize(TRAY_QUOTA_WIDTH, height))
      .catch(() => undefined);
  }, [rows, error, arrangeTick]);

  return (
    <div className="tray-quota-app">
      <article ref={panelRef} className="panel official-quota-panel">
        <div className="panel-head">
          <div className="official-quota-heading">
            <h2>官方额度</h2>
            <span className="muted official-quota-refresh-hint">拖动排序 · 点标题折叠</span>
          </div>
        </div>
        {error ? (
          <EmptyState compact icon="clock" title="无法读取官方额度" hint={error} />
        ) : rows.length === 0 ? (
          <EmptyState
            compact
            icon="clock"
            title={quota ? "所选账号均已隐藏" : "正在读取官方额度…"}
            hint={quota ? "在主窗口「配置显示」里打开要看的账号" : "先显示上次缓存"}
          />
        ) : (
          <OfficialQuotaList
            rows={rows}
            staleAfterMinutes={quota?.stale_after_minutes}
            compactReset
            arrangeable
            onArrange={() => setArrangeTick((tick) => tick + 1)}
          />
        )}
      </article>
    </div>
  );
}
