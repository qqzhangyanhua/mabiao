import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import {
  officialQuotaSettingsRefreshNote,
  officialQuotaUndetectedNote,
} from "../lib/officialQuotaDisplay";
import type { OfficialQuotaDto, OfficialQuotaHookDto } from "../types";
import { OfficialQuotaPlanMark, QuotaFreshnessMark, useTickingNow } from "./OfficialQuotaPanel";
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
  const [busy, setBusy] = useState<"idle" | "refresh" | "hook" | "alerts">("idle");
  const nowMs = useTickingNow();
  const alertsEnabled = quota?.alerts_enabled ?? true;
  const undetectedNote = quota ? officialQuotaUndetectedNote(quota.undetected) : null;

  useEffect(() => {
    void invoke<OfficialQuotaHookDto>("get_official_quota_hook").then(setHook).catch(onError);
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
      // 配置文件是整份覆盖写入，漏带 hidden_providers 会把「配置显示」里
      // 关掉的账号悄悄重新打开——这里必须带上当前值再改 alerts_enabled。
      await invoke("save_official_quota_config", {
        config: {
          alerts_enabled: !alertsEnabled,
          hidden_providers: quota?.hidden_providers ?? [],
        },
      });
      const next = await invoke<OfficialQuotaDto>("get_official_quota");
      onQuota(next);
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
            <li
              key={row.provider}
              className={
                row.freshness === "official"
                  ? "tone-ok"
                  : row.freshness === "stale"
                    ? "tone-warn"
                    : "tone-idle"
              }
            >
              <strong>
                <SourceLabel source={row.provider} fallback={row.application} size={14} />
              </strong>
              {row.plan ? <OfficialQuotaPlanMark plan={row.plan} /> : null}
              <QuotaFreshnessMark
                freshness={row.freshness}
                capturedAt={row.captured_at}
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
