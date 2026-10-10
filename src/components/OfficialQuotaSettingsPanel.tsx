import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useTickingNow } from "../hooks/useTickingNow";
import {
  DEFAULT_OFFICIAL_QUOTA_CONFIG,
  formatAlertThresholdsInput,
  parseAlertThresholdsInput,
  persistOfficialQuotaConfig,
} from "../lib/officialQuotaConfig";
import {
  officialQuotaRowTone,
  officialQuotaSettingsRefreshNote,
  officialQuotaUndetectedNote,
} from "../lib/officialQuotaDisplay";
import type { OfficialQuotaConfig, OfficialQuotaDto, OfficialQuotaHookDto } from "../types";
import { OfficialQuotaPlanMark, QuotaFreshnessMark } from "./OfficialQuotaPanel";
import { SourceLabel } from "./SourceIcon";
import { Button } from "./ui/Button";

export function OfficialQuotaSettingsPanel({
  quota,
  onQuota,
  onError,
}: {
  quota: OfficialQuotaDto | null;
  onQuota: (value: OfficialQuotaDto) => void;
  onError: (error: unknown) => void;
}) {
  const [hook, setHook] = useState<OfficialQuotaHookDto | null>(null);
  const [config, setConfig] = useState<OfficialQuotaConfig>(DEFAULT_OFFICIAL_QUOTA_CONFIG);
  const [thresholdInput, setThresholdInput] = useState("80, 100");
  const [busy, setBusy] = useState<"idle" | "refresh" | "hook" | "alerts" | "save">("idle");
  const nowMs = useTickingNow();
  const alertsEnabled = quota?.alerts_enabled ?? config.alerts_enabled;
  const undetectedNote = quota ? officialQuotaUndetectedNote(quota.undetected) : null;

  useEffect(() => {
    void invoke<OfficialQuotaHookDto>("get_official_quota_hook").then(setHook).catch(onError);
    void invoke<OfficialQuotaConfig>("get_official_quota_config")
      .then((next) => {
        setConfig(next);
        setThresholdInput(formatAlertThresholdsInput(next.alert_thresholds));
      })
      .catch(onError);
  }, [onError]);

  async function refresh() {
    setBusy("refresh");
    try {
      onQuota(await invoke<OfficialQuotaDto>("refresh_official_quota"));
    } catch (error) {
      onError(error);
    } finally {
      setBusy("idle");
    }
  }

  async function applyHook() {
    setBusy("hook");
    try {
      setHook(await invoke<OfficialQuotaHookDto>("apply_official_quota_hook"));
    } catch (error) {
      onError(error);
    } finally {
      setBusy("idle");
    }
  }

  async function toggleAlerts() {
    setBusy("alerts");
    try {
      // 整份覆盖写入：先读当前配置再改 alerts_enabled，避免冲掉自定义档位。
      const next = await persistOfficialQuotaConfig({ alerts_enabled: !alertsEnabled });
      setConfig(next);
      onQuota(await invoke<OfficialQuotaDto>("get_official_quota"));
    } catch (error) {
      onError(error);
    } finally {
      setBusy("idle");
    }
  }

  async function saveAlertSettings() {
    setBusy("save");
    try {
      const next = await persistOfficialQuotaConfig({
        alert_thresholds: parseAlertThresholdsInput(thresholdInput),
        reset_reminder_hours: config.reset_reminder_hours,
        reset_reminder_max_used_percent: config.reset_reminder_max_used_percent,
      });
      setConfig(next);
      setThresholdInput(formatAlertThresholdsInput(next.alert_thresholds));
    } catch (error) {
      onError(error);
    } finally {
      setBusy("idle");
    }
  }

  return (
    <section className="panel" id="settings-official-quota">
      <div className="panel-head">
        <div>
          <h2>官方额度</h2>
          <p className="panel-note">
            Claude 通过 statusline 捕获本机官方百分比；Codex 问本机 app-server；Cursor 读取本机
            Cursor 客户端登录态打限额接口；Grok 读取本机 <code>~/.grok/auth.json</code> 打 CLI
            限额接口。已有 Claude statusLine 不会被覆盖。
            {quota ? ` ${officialQuotaSettingsRefreshNote(quota.stale_after_minutes)}` : ""}
          </p>
        </div>
        <div className="row-actions">
          <Button disabled={busy !== "idle"} onClick={() => void refresh()}>
            {busy === "refresh" ? "刷新中…" : "刷新额度"}
          </Button>
          <Button variant="accent" disabled={busy !== "idle"} onClick={() => void toggleAlerts()}>
            {alertsEnabled ? "关闭额度告警" : "开启额度告警"}
          </Button>
        </div>
      </div>
      {quota ? (
        <ul className="official-quota-status">
          {quota.rows.map((row) => (
            <li key={row.provider} className={`tone-${officialQuotaRowTone(row)}`}>
              <strong>
                <SourceLabel source={row.provider} fallback={row.application} size={14} />
              </strong>
              {row.plan ? <OfficialQuotaPlanMark plan={row.plan} /> : null}
              <QuotaFreshnessMark
                row={row}
                staleAfterMinutes={quota.stale_after_minutes}
                nowMs={nowMs}
              />
              <em>
                {row.todo ??
                  row.error ??
                  (row.windows.length > 0 ? `${row.windows.length} 个窗口` : "等待捕获")}
              </em>
            </li>
          ))}
        </ul>
      ) : null}
      {undetectedNote ? <p className="panel-note">{undetectedNote}</p> : null}
      <div className="official-quota-alert-fields">
        <label className="official-quota-alert-field">
          <span>告警档位（%）</span>
          <input
            value={thresholdInput}
            onChange={(event) => setThresholdInput(event.target.value)}
            placeholder="80, 100"
          />
          <em>默认 80、100。逗号分隔，1–100。旧配置缺字段时仍用这两档。</em>
        </label>
        <label className="official-quota-alert-field">
          <span>重置前提醒（小时）</span>
          <input
            type="number"
            min={0}
            max={168}
            value={config.reset_reminder_hours}
            onChange={(event) =>
              setConfig((current) => ({
                ...current,
                reset_reminder_hours: Number.parseInt(event.target.value, 10) || 0,
              }))
            }
          />
          <em>距重置不足此时长、且已用低于下方比例时提醒一次。0 表示关闭。5 小时窗不提醒。</em>
        </label>
        <label className="official-quota-alert-field">
          <span>「还剩很多」上限（已用 %）</span>
          <input
            type="number"
            min={0}
            max={100}
            value={config.reset_reminder_max_used_percent}
            onChange={(event) =>
              setConfig((current) => ({
                ...current,
                reset_reminder_max_used_percent: Number.parseInt(event.target.value, 10) || 0,
              }))
            }
          />
          <em>已用低于此值才提醒。默认 50。每个重置周期只弹一次。</em>
        </label>
        <Button disabled={busy !== "idle"} onClick={() => void saveAlertSettings()}>
          {busy === "save" ? "保存中…" : "保存告警设置"}
        </Button>
      </div>
      {hook ? (
        <div className="official-quota-hook">
          <p className="panel-note">
            Claude 设置：<code>{hook.settings_path}</code>
            {hook.already_configured ? " · 已写入本应用 hook" : null}
            {hook.conflict ? " · 已有自定义 statusLine，未覆盖" : null}
          </p>
          {hook.conflict ? (
            <p className="panel-note">
              当前 command：<code>{hook.conflict_command}</code>
              。请自行把下面的命令接到现有 hook 里。
            </p>
          ) : null}
          <pre className="official-quota-snippet">{hook.snippet}</pre>
          <Button
            variant="accent"
            disabled={busy !== "idle" || hook.conflict || hook.already_configured}
            onClick={() => void applyHook()}
          >
            {hook.already_configured
              ? "已配置"
              : hook.conflict
                ? "已有 hook，未写入"
                : "预览确认后写入"}
          </Button>
        </div>
      ) : null}
    </section>
  );
}
