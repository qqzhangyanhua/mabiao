import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { humanStatus } from "../lib/format";
import { autoPushProgressLine } from "../lib/pushRange";
import type { AutoPushDto } from "../types";
import { Select } from "./ui/Select";

/** 后台推送的结果与登录状态会在页面开着时变，隔一会儿重读（只读两个本机小文件）。 */
const REFRESH_MS = 10_000;

const OPTIONS = [
  { value: "off", label: "关闭" },
  { value: "on", label: "每天推送昨天" },
];

/**
 * 每日自动推送的开关。开关与进度都在 Rust；这里只显示、只提交开关。
 * 登录不可用时的提示也由 Rust 给出，所以这里不重复判断登录状态。
 */
export function AutoPushPanel() {
  const [auto, setAuto] = useState<AutoPushDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    const load = () =>
      invoke<AutoPushDto>("get_auto_push")
        .then((loaded) => current && setAuto(loaded))
        .catch((cause: unknown) => current && setError(humanStatus(cause)));
    void load();
    const timer = window.setInterval(() => void load(), REFRESH_MS);
    return () => {
      current = false;
      window.clearInterval(timer);
    };
  }, []);

  async function change(value: string) {
    setBusy(true);
    setError(null);
    try {
      setAuto(await invoke<AutoPushDto>("set_auto_push", { enabled: value === "on" }));
    } catch (cause) {
      setError(humanStatus(cause));
    } finally {
      setBusy(false);
    }
  }

  const progress = auto ? autoPushProgressLine(auto) : null;

  return (
    <section className="panel" id="settings-auto-push">
      <div className="panel-head">
        <div>
          <h2>每日自动推送</h2>
          <p className="panel-note">
            默认关闭。打开后，应用运行时每天把前一天的对话与消耗推送到远程服务，规则与手动推送相同（同样会打码、读不全的会话整场跳过），不弹预览，结果记在推送历史里。应用没开而漏掉的天，下次启动会补推（最多补近
            31 天）。管理员可以查看你推送的全部正文。
          </p>
        </div>
      </div>
      <div className="settings-rows">
        <div className="settings-row">
          <div className="settings-row-copy">
            <h3>自动推送</h3>
            <p>{progress ?? "尚未推送过。"}</p>
          </div>
          <Select
            ariaLabel="每日自动推送"
            value={auto?.enabled ? "on" : "off"}
            options={OPTIONS}
            disabled={busy || auto === null}
            onChange={(value) => void change(value)}
          />
        </div>
      </div>
      {auto?.login_notice ? (
        <p className="panel-note tone-warn" role="alert">
          {auto.login_notice}
        </p>
      ) : null}
      {auto?.last_error ? (
        <p className="panel-note tone-danger" role="alert">
          上次自动推送没有推干净：{auto.last_error}
        </p>
      ) : null}
      {error ? (
        <p className="panel-note tone-danger" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}
