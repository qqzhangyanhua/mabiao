import { AUTO_REFRESH_OPTIONS } from "../hooks/usage/constants";
import type { ThemeMode } from "../hooks/useTheme";
import { Icon } from "../icons";
import type { ThemeOption } from "./type";
import { Select } from "./ui/Select";

const THEME_OPTIONS: ThemeOption[] = [
  { value: "system", label: "跟随系统", icon: "monitor", note: "自动匹配系统外观" },
  { value: "light", label: "浅色", icon: "sun", note: "始终使用浅色界面" },
  { value: "dark", label: "深色", icon: "moon", note: "始终使用深色界面" },
];

export function AppearanceSettingsPanel({
  themeMode,
  autoRefresh,
  onThemeModeChange,
  onAutoRefreshChange,
}: {
  themeMode: ThemeMode;
  autoRefresh: string;
  onThemeModeChange: (mode: ThemeMode) => void;
  onAutoRefreshChange: (value: string) => void;
}) {
  return (
    <section className="panel" id="settings-appearance">
      <div className="panel-head">
        <div>
          <h2>通用</h2>
          <p className="panel-note">外观、自动刷新与概览显示保存在本机，不写入用量缓存。</p>
        </div>
      </div>
      <div className="settings-rows">
        <div className="settings-block">
          <div className="settings-row-copy">
            <h3>主题</h3>
            <p>只改界面颜色，不影响统计口径。</p>
          </div>
          <div className="theme-choice" role="radiogroup" aria-label="外观主题">
            {THEME_OPTIONS.map((option) => {
              const active = themeMode === option.value;
              return (
                <button
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  className={active ? "theme-choice-btn active" : "theme-choice-btn"}
                  onClick={() => onThemeModeChange(option.value)}
                >
                  <Icon name={option.icon} size={16} />
                  <strong>{option.label}</strong>
                  <small>{option.note}</small>
                </button>
              );
            })}
          </div>
        </div>
        <div className="settings-row">
          <div className="settings-row-copy">
            <h3>自动刷新</h3>
            <p>按间隔重新摄取本机会话数据。Cursor 账号用量不在此列，可在 Cursor 页单独打开。</p>
          </div>
          <Select
            ariaLabel="自动刷新间隔"
            value={autoRefresh}
            options={AUTO_REFRESH_OPTIONS}
            onChange={onAutoRefreshChange}
          />
        </div>
      </div>
    </section>
  );
}
