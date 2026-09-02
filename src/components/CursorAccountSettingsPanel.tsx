import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES } from "../hooks/usage/constants";
import { formatClock, humanStatus } from "../lib/format";
import { Button } from "./ui/Button";

/**
 * 设置页的 Cursor 账号用量入口：本机 Cursor 登录态、独立自动刷新开关、清空缓存。
 * 凭证没有手动通路——Cursor 自己会续期。清空不联网，也不动本机消耗记录。
 */
type CursorCredentialStatus = {
  source: "local" | "none";
  email: string | null;
  expires_at: string | null;
  local_expired: boolean;
};

function describeCredential(status: CursorCredentialStatus): string {
  if (status.source === "local") {
    const who = status.email ? `（${status.email}）` : "";
    const until = status.expires_at ? `，有效期至 ${formatClock(status.expires_at)}` : "";
    return `已读取本机 Cursor 客户端登录态${who}${until}。`;
  }
  return status.local_expired
    ? "本机 Cursor 登录态已过期，请在 Cursor 客户端重新登录。"
    : "未找到本机 Cursor 登录态，请确认装了 Cursor 客户端并已登录。";
}

export function CursorAccountSettingsPanel({
  autoRefresh,
  onAutoRefreshChange,
}: {
  autoRefresh: boolean;
  onAutoRefreshChange: (value: boolean) => void;
}) {
  const [status, setStatus] = useState<CursorCredentialStatus | null>(null);
  const [busy, setBusy] = useState<"idle" | "clearing">("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    invoke<CursorCredentialStatus>("get_cursor_credential_status")
      .then((next) => {
        if (alive) {
          setStatus(next);
        }
      })
      .catch((err: unknown) => {
        if (alive) {
          setError(humanStatus(err));
        }
      });
    return () => {
      alive = false;
    };
  }, []);

  async function handleClearCache() {
    setBusy("clearing");
    setMessage(null);
    setError(null);
    try {
      await invoke("clear_cursor_account_usage");
      setMessage("已清空 Cursor 账号用量缓存，本机消耗记录未改动；下次刷新将重新拉全量");
    } catch (err: unknown) {
      setError(humanStatus(err));
    } finally {
      setBusy("idle");
    }
  }

  return (
    <section className="panel" id="settings-cursor-account">
      <div className="panel-head">
        <div>
          <h2>Cursor 账号用量</h2>
          <p className="panel-note">
            凭证读本机 Cursor 客户端的登录态（只读，不写 Cursor 任何文件），跟着客户端自动续期，
            不落配置文件。账号用量不跟本机会话的 1/5/10 分钟定时器，可在下方单独打开自动刷新。
            清空只删这张独立缓存表，不触发联网，更不动本机消耗记录。
          </p>
          {status ? (
            <p className="panel-note" role="status">
              {describeCredential(status)}
            </p>
          ) : null}
        </div>
        <Button variant="danger" disabled={busy !== "idle"} onClick={() => void handleClearCache()}>
          {busy === "clearing" ? "清空中…" : "清空账号用量缓存"}
        </Button>
      </div>
      <div className="settings-rows">
        <div className="settings-row">
          <div className="settings-row-copy">
            <h3>自动刷新</h3>
            <p>
              独立开关，不跟本机会话定时器。开启后每 {CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES}{" "}
              分钟联网拉取账号用量，仍不进本机 KPI / 5 小时窗。
            </p>
          </div>
          <button
            type="button"
            className={["custom-quota-switch", autoRefresh ? "is-on" : "is-off"].join(" ")}
            role="switch"
            aria-checked={autoRefresh}
            aria-label="Cursor 账号用量自动刷新"
            onClick={() => onAutoRefreshChange(!autoRefresh)}
          >
            {autoRefresh ? "已启用" : "已停用"}
          </button>
        </div>
      </div>
      {message ? (
        <p className="panel-note preset-message" role="status">
          {message}
        </p>
      ) : null}
      {error ? (
        <p className="panel-note snapshot-error" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}
