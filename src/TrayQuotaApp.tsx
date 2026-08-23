import { LogicalSize } from "@tauri-apps/api/dpi";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { EmptyState } from "./components/EmptyState";
import { OfficialQuotaList } from "./components/OfficialQuotaPanel";
import { Button } from "./components/ui/Button";
import { useTheme } from "./hooks/useTheme";
import { Icon } from "./icons";
import { visibleOfficialQuotaRows } from "./lib/overviewLayout";
import { readSectionOpen, writeSectionOpen } from "./lib/sectionCollapse";
import {
  clampTrayQuotaWindowHeight,
  TRAY_QUOTA_WIDTH,
} from "./lib/trayQuotaLayout";
import type { OfficialQuotaDto } from "./types";

const TRAY_QUOTA_SECTION_ID = "tray-official-quota";

export default function TrayQuotaApp() {
  useTheme();
  const [quota, setQuota] = useState<OfficialQuotaDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [arrangeTick, setArrangeTick] = useState(0);
  const [open, setOpen] = useState(() => readSectionOpen(TRAY_QUOTA_SECTION_ID, true));
  const [busyProvider, setBusyProvider] = useState<string | null>(null);
  const panelRef = useRef<HTMLElement>(null);

  // 面板空间小，不方便像主窗口那样先弹一句「还要等 N 分钟」再让用户决定要不要硬刷——
  // 点了就是要现在就试一次，所以这里走跳过退避冷却的强制刷新。
  async function refreshProvider(provider: string) {
    setBusyProvider(provider);
    try {
      setQuota(
        await invoke<OfficialQuotaDto>("refresh_official_quota_provider_force", { provider }),
      );
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusyProvider(null);
    }
  }

  function toggleOpen() {
    setOpen((prev) => {
      const next = !prev;
      writeSectionOpen(TRAY_QUOTA_SECTION_ID, next);
      return next;
    });
  }

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
  }, [rows, error, arrangeTick, open]);

  const collapsedSummary = error
    ? "读取失败"
    : quota
      ? rows.length > 0
        ? `${rows.length} 个账号`
        : "所选账号均已隐藏"
      : "正在读取…";

  return (
    <div className="tray-quota-app">
      <article
        ref={panelRef}
        className={["panel", "official-quota-panel", "collapsible-section", open ? "is-open" : "is-collapsed"].join(
          " ",
        )}
      >
        <div className="panel-head collapsible-head">
          <div className="official-quota-heading">
            <h2>官方额度</h2>
            {open ? (
              <span className="muted official-quota-refresh-hint">
                拖动排序 · 点标题折叠 · 图标强制刷新
              </span>
            ) : null}
          </div>
          <div className="collapsible-actions">
            {open ? null : <span className="muted collapsible-summary">{collapsedSummary}</span>}
            <Button
              variant="icon"
              className="collapsible-toggle"
              aria-expanded={open}
              aria-label={open ? "收起官方额度" : "展开官方额度"}
              title={open ? "收起" : "展开"}
              onClick={toggleOpen}
            >
              <Icon name="chevron" size={13} className="caret" />
            </Button>
          </div>
        </div>
        {open ? (
          error ? (
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
              busyProvider={busyProvider}
              onRefresh={(provider) => void refreshProvider(provider)}
              onArrange={() => setArrangeTick((tick) => tick + 1)}
            />
          )
        ) : null}
      </article>
    </div>
  );
}
