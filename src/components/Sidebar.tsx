import { useEffect, useState } from "react";
import { Icon, type IconName } from "../icons";
import { navLabel, viewTitle } from "../lib/viewTitle";
import type { View } from "../types";
import appIcon from "../../src-tauri/icons/icon.png";

export { viewTitle };

const SIDEBAR_COLLAPSED_KEY = "mabiao:sidebar-collapsed";

function loadCollapsed(): boolean {
  try {
    return window.localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

const navGroups: { label: string; items: { id: View; icon: IconName }[] }[] = [
  {
    label: "用量",
    items: [
      { id: "overview", icon: "overview" },
      { id: "trend", icon: "trend" },
      { id: "model", icon: "model" },
      { id: "project", icon: "project" },
      { id: "application", icon: "source" },
      { id: "provider", icon: "provider" },
      { id: "worktime", icon: "clock" },
    ],
  },
  {
    label: "对话",
    items: [{ id: "conversations", icon: "chat" }],
  },
  {
    label: "Cursor",
    items: [
      { id: "cursor", icon: "cursor" },
      { id: "cursor-sessions", icon: "sessions" },
    ],
  },
  {
    label: "系统",
    items: [
      { id: "instructions", icon: "instruction" },
      { id: "settings", icon: "settings" },
    ],
  },
];

type ConnTone = "ok" | "busy" | "partial" | "off";

const CONN_TITLE: Record<ConnTone, string> = {
  ok: "连接正常",
  busy: "正在同步",
  partial: "部分失败",
  off: "连接异常",
};

function connTone(connected: boolean, busy: boolean, partial: boolean): ConnTone {
  if (!connected) {
    return "off";
  }
  if (busy) {
    return "busy";
  }
  if (partial) {
    return "partial";
  }
  return "ok";
}

function liveDotClass(tone: ConnTone): string {
  return tone === "ok" ? "live-dot" : `live-dot ${tone}`;
}

export function Sidebar({
  view,
  busy,
  connected,
  partial,
  status,
  onNavigate,
}: {
  view: View;
  busy: boolean;
  connected: boolean;
  partial: boolean;
  status: string;
  onNavigate: (view: View) => void;
}) {
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const tone = connTone(connected, busy, partial);
  const toneTitle = CONN_TITLE[tone];

  useEffect(() => {
    try {
      window.localStorage.setItem(SIDEBAR_COLLAPSED_KEY, collapsed ? "1" : "0");
    } catch {
      // localStorage 不可用时忽略，仅影响下次启动是否记住折叠状态
    }
  }, [collapsed]);

  return (
    <aside className={collapsed ? "sidebar collapsed" : "sidebar"}>
      <div className="brand">
        <img src={appIcon} alt="" width={34} height={34} className="brand-logo" />
        <div className={collapsed ? "sr-only" : undefined}>
          <div className="brand-name">本机用量</div>
          <div className="brand-meta">
            Token 统计
            <span className="badge">本地</span>
          </div>
        </div>
      </div>
      <nav className="nav">
        {navGroups.map((group) => (
          <div className="nav-group" key={group.label}>
            <div className={collapsed ? "sr-only" : "nav-group-label"}>{group.label}</div>
            {group.items.map((item) => {
              const label = navLabel(item.id);
              return (
                <button
                  key={item.id}
                  className={view === item.id ? "nav-btn active" : "nav-btn"}
                  onClick={() => onNavigate(item.id)}
                  title={collapsed ? label : undefined}
                >
                  <Icon name={item.icon} size={16} />
                  <span className={collapsed ? "sr-only" : undefined}>{label}</span>
                </button>
              );
            })}
          </div>
        ))}
      </nav>
      <button
        type="button"
        className="sidebar-collapse-btn"
        onClick={() => setCollapsed((value) => !value)}
        aria-pressed={collapsed}
        aria-label={collapsed ? "展开侧边栏" : "收起侧边栏"}
        title={collapsed ? "展开侧边栏" : "收起侧边栏"}
      >
        <Icon name="chevron" size={14} className={collapsed ? "flip" : undefined} />
        <span className={collapsed ? "sr-only" : undefined}>收起</span>
      </button>
      {!collapsed ? (
        <div className="sidebar-foot">
          <div className="conn-card">
            <span className={liveDotClass(tone)} aria-hidden="true" />
            <div className="conn-copy">
              <div className={`conn-title ${tone}`}>{toneTitle}</div>
              <div className="conn-sub">{status}</div>
            </div>
          </div>
          <div className="version" title="数字键切页 · R 刷新 · Esc 清空筛选">
            版本 {__APP_VERSION__} · 快捷键 R / 1-0
          </div>
        </div>
      ) : (
        <div className="sidebar-foot collapsed-foot">
          <span
            className={liveDotClass(tone)}
            title={`${toneTitle} · ${status}`}
            aria-label={`${toneTitle}：${status}`}
          />
        </div>
      )}
    </aside>
  );
}
