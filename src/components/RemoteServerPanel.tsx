import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { formatClock, humanStatus } from "../lib/format";
import type { RemoteServerDto } from "../types";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";

/** 防抖：边打边校验地址，但不必每敲一个字符都发一次命令。 */
const URL_CHECK_DEBOUNCE_MS = 300;

/**
 * 地址是否合法由 Rust 判定（强制 https，回环地址例外），这里只显示结果，
 * 这样「保存前的提示」与「真正联网前的校验」永远是同一份规则。
 * 结果连同它是对着哪段输入算的一起收着：输入变了而新结果还没回来时，不显示旧结论。
 */
function useUrlProblem(baseUrl: string): string | null {
  const typed = baseUrl.trim();
  const [checked, setChecked] = useState<{ typed: string; problem: string | null } | null>(null);
  useEffect(() => {
    if (typed === "") {
      return;
    }
    let current = true;
    const timer = window.setTimeout(() => {
      void invoke<string>("validate_remote_server_url", { baseUrl: typed })
        .then(() => current && setChecked({ typed, problem: null }))
        .catch((cause: unknown) => current && setChecked({ typed, problem: humanStatus(cause) }));
    }, URL_CHECK_DEBOUNCE_MS);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [typed]);
  return typed !== "" && checked?.typed === typed ? checked.problem : null;
}

export function RemoteServerPanel() {
  const [server, setServer] = useState<RemoteServerDto | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [account, setAccount] = useState("");
  // 密码只活在这个输入框和登录那一次调用里：成功后立刻清空，不进任何持久状态。
  const [password, setPassword] = useState("");
  const [deviceName, setDeviceName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const urlProblem = useUrlProblem(baseUrl);

  function adopt(next: RemoteServerDto) {
    setServer(next);
    setBaseUrl(next.base_url);
    setAccount(next.account);
    setDeviceName(next.device_name);
  }

  useEffect(() => {
    let current = true;
    void (async () => {
      try {
        const loaded = await invoke<RemoteServerDto>("get_remote_server");
        if (!current) {
          return;
        }
        adopt(loaded);
        if (loaded.state === "logged_in") {
          // 向服务端确认这次登录还被认；断网只提示原因，不改本机登录态。
          try {
            const verified = await invoke<RemoteServerDto>("verify_remote_server");
            if (current) {
              adopt(verified);
            }
          } catch (cause) {
            if (current) {
              setError(humanStatus(cause));
            }
          }
        }
      } catch (cause) {
        if (current) {
          setError(humanStatus(cause));
        }
      }
    })();
    return () => {
      current = false;
    };
  }, []);

  async function run(action: () => Promise<RemoteServerDto>, afterSuccess?: () => void) {
    setBusy(true);
    setError(null);
    try {
      adopt(await action());
      afterSuccess?.();
    } catch (cause) {
      setError(humanStatus(cause));
    } finally {
      setBusy(false);
    }
  }

  const loggedIn = server?.state === "logged_in";
  const canLogin =
    !busy && baseUrl.trim() !== "" && account.trim() !== "" && password !== "" && !urlProblem;

  return (
    <section className="panel" id="settings-remote-server">
      <div className="panel-head">
        <div>
          <h2>远程服务</h2>
          <p className="panel-note">
            登录团队的远程服务，之后才能在对话记录页把对话与消耗推送上去。必须是 https
            地址（本机 localhost 可用 http）。密码只用来换登录凭证，不会保存；凭证单独存一份仅本人可读的文件，不进备份。
            管理员可以查看你推送的全部正文。
          </p>
        </div>
        {loggedIn ? (
          <div className="row-actions">
            <Button
              disabled={busy}
              onClick={() => void run(() => invoke<RemoteServerDto>("remote_server_logout"))}
            >
              退出登录
            </Button>
          </div>
        ) : null}
      </div>

      {server?.notice ? (
        <p className="panel-note tone-warn" role="alert">
          {server.notice}
        </p>
      ) : null}
      {error ? (
        <p className="panel-note tone-danger" role="alert">
          {error}
        </p>
      ) : null}

      {loggedIn ? (
        <div className="remote-server-status">
          <strong>{server.account}</strong>
          <span className="muted">{server.role === "admin" ? "管理员" : "成员"}</span>
          <code>{server.base_url}</code>
          <span className="muted">登录有效至 {formatClock(server.expires_at)}</span>
        </div>
      ) : (
        <form
          className="custom-quota-form"
          onSubmit={(event) => {
            event.preventDefault();
            if (!canLogin) {
              return;
            }
            void run(
              () =>
                invoke<RemoteServerDto>("remote_server_login", {
                  input: {
                    base_url: baseUrl,
                    account,
                    password,
                    device_name: deviceName.trim() === "" ? null : deviceName,
                  },
                }),
              () => setPassword(""),
            );
          }}
        >
          <Field
            label="服务地址"
            placeholder="https://mabiao.example.com"
            value={baseUrl}
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => setBaseUrl(event.target.value)}
          />
          {urlProblem ? <p className="panel-note tone-danger">{urlProblem}</p> : null}
          <Field
            label="远程账号"
            value={account}
            autoComplete="username"
            spellCheck={false}
            onChange={(event) => setAccount(event.target.value)}
          />
          <Field
            label="密码"
            type="password"
            value={password}
            autoComplete="current-password"
            onChange={(event) => setPassword(event.target.value)}
          />
          {server?.device_id ? null : (
            <Field
              label="设备名（可留空，默认用本机主机名）"
              value={deviceName}
              onChange={(event) => setDeviceName(event.target.value)}
            />
          )}
          <div className="row-actions">
            <Button type="submit" variant="accent" disabled={!canLogin}>
              {busy ? "登录中…" : server?.state === "not_logged_in" || !server ? "登录" : "重新登录"}
            </Button>
          </div>
        </form>
      )}

      {server?.device_id ? (
        <div className="remote-server-device">
          <Field
            label="设备名"
            value={deviceName}
            onChange={(event) => setDeviceName(event.target.value)}
          />
          <Button
            disabled={busy || deviceName.trim() === "" || deviceName.trim() === server.device_name}
            onClick={() =>
              void run(() =>
                invoke<RemoteServerDto>("rename_remote_device", { name: deviceName }),
              )
            }
          >
            保存设备名
          </Button>
          <span className="muted remote-server-id">设备 ID {server.device_id}</span>
        </div>
      ) : null}
    </section>
  );
}
