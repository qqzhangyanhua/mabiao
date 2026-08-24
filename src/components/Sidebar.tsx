import { useEffect, useState } from "react";
import { Icon, type IconName } from "../icons";
import type { View } from "../types";
import appIcon from "../../src-tauri/icons/icon.png";

const SIDEBAR_COLLAPSED_KEY = "mabiao:sidebar-collapsed";

function loadCollapsed(): boolean {
  try {
    return window.localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

const navGroups: { label: string; items: { id: View; label: string; icon: IconName }[] }[] = [
  {
    label: "用量",
    items: [
      { id: "overview", label: "概览", icon: "overview" },
      { id: "trend", label: "使用统计", icon: "trend" },
      { id: "model", label: "模型统计", icon: "model" },
      { id: "project", label: "项目统计", icon: "project" },
      { id: "application", label: "应用统计", icon: "source" },
      { id: "provider", label: "Provider", icon: "provider" },
      { id: "worktime", label: "工作时间线", icon: "clock" },
    ],
  },
  {
    label: "对话",
    items: [{ id: "conversations", label: "对话记录", icon: "chat" }],
  },
  {
    label: "Cursor",
    items: [
      { id: "cursor", label: "代码量", icon: "cursor" },
      { id: "cursor-sessions", label: "会话", icon: "sessions" },
    ],
  },
  {
    label: "系统",
    items: [
      { id: "instructions", label: "全局指令", icon: "instruction" },
      { id: "settings", label: "设置", icon: "settings" },
    ],
  },
];

export function Sidebar({
  view,
  busy,
  connected,
  status,
  onNavigate,
}: {
  view: View;
  busy: boolean;
  connected: boolean;
  status: string;
  onNavigate: (view: View) => void;
}) {
  const [collapsed, setCollapsed] = useState(loadCollapsed);

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
            {group.items.map((item) => (
              <button
                key={item.id}
                className={view === item.id ? "nav-btn active" : "nav-btn"}
                onClick={() => onNavigate(item.id)}
                title={collapsed ? item.label : undefined}
              >
                <Icon name={item.icon} size={16} />
                <span className={collapsed ? "sr-only" : undefined}>{item.label}</span>
              </button>
            ))}
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
            <span className={connected ? "live-dot" : "live-dot off"} />
            <div>
              <div className="conn-title">
                {connected ? (busy ? "正在同步" : "连接正常") : "连接异常"}
              </div>
              <div className="conn-sub" title={status}>
                {status}
              </div>
            </div>
          </div>
          <div className="version" title="数字键切页 · R 刷新 · Esc 清空筛选">
            版本 0.1.0 · 快捷键 R / 1-0
          </div>
        </div>
      ) : (
        <div className="sidebar-foot collapsed-foot">
          <span
            className={connected ? "live-dot" : "live-dot off"}
            title={connected ? "连接正常" : "连接异常"}
          />
        </div>
      )}
    </aside>
  );
}

export function viewTitle(view: View): { title: string; subtitle: string } {
  switch (view) {
    case "overview":
      return { title: "概览", subtitle: "全局 Token 使用概览" };
    case "trend":
      return { title: "使用统计", subtitle: "按时间查看 Token 消耗" };
    case "conversations":
      return { title: "对话记录", subtitle: "本地会话正文，含 Cursor Agent" };
    case "model":
      return { title: "模型统计", subtitle: "按模型拆分 Token 与费用" };
    case "project":
      return { title: "项目统计", subtitle: "按项目拆分 Token 与费用" };
    case "application":
      return { title: "应用统计", subtitle: "趋势、项目交叉与效率指标" };
    case "provider":
      return { title: "Provider", subtitle: "按官方 / 中转渠道拆分" };
    case "worktime":
      return { title: "工作时间线", subtitle: "所选日期的工作片段分布" };
    case "cursor":
      return { title: "Cursor 代码量", subtitle: "独立口径，不计入 Token" };
    case "cursor-sessions":
      return { title: "Cursor 会话", subtitle: "Agent 行为统计，不计入 Token" };
    case "instructions":
      return { title: "全局指令", subtitle: "跨来源的全局指令与体检" };
    case "settings":
      return { title: "设置", subtitle: "外观、数据源与单价" };
  }
}
