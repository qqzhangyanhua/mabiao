import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { humanStatus } from "../lib/format";
import { OFFICIAL_QUOTA_PROVIDER_IDS, officialQuotaProviderLabel } from "../lib/overviewLayout";
import type { OfficialQuotaDto } from "../types";
import { Button } from "./ui/Button";

type QuotaAccountSource = "local_snapshot" | "profile_dir" | "oauth";

type QuotaAccountDto = {
  id: string;
  provider: string;
  name: string;
  hint: string;
  source: QuotaAccountSource;
  profile_dir?: string | null;
  enabled: boolean;
};

type QuotaAccountPanelDto = {
  accounts: QuotaAccountDto[];
};

type SavedQuotaAccountDto = {
  saved_id: string;
  enabled: boolean;
  panel: QuotaAccountPanelDto;
};

type SaveQuotaAccount = {
  id: string;
  name: string | null;
  enabled: boolean | null;
};

type SaveProfileDirAccount = {
  id: string | null;
  provider: string;
  name: string;
  profile_dir: string;
};

type CopilotDeviceStart = {
  session_id: string;
  user_code: string;
  verification_url: string;
  interval_seconds: number;
};

type CopilotDevicePoll =
  | { status: "pending"; interval_seconds: number }
  | { status: "authorized"; saved: SavedQuotaAccountDto };

type DevicePrompt = {
  sessionId: string;
  userCode: string;
  verificationUrl: string;
  intervalSeconds: number;
};

type CodexPkceStart = {
  session_id: string;
  authorize_url: string;
  redirect_uri: string;
  listening: boolean;
  paste_hint?: string | null;
};

type CodexPkcePoll =
  | { status: "pending" }
  | { status: "authorized"; saved: SavedQuotaAccountDto };

type CodexPrompt = {
  sessionId: string;
  authorizeUrl: string;
  listening: boolean;
  pasteHint: string | null;
};

const CANCELLED = "已取消设备码授权，没有保存额度账号";
const CODEX_CANCELLED = "已取消浏览器授权，没有保存额度账号";

function accountSourceLabel(source: string, provider: string): string {
  if (source === "oauth" && provider === "codex") return "浏览器授权";
  if (source === "oauth") return "设备码授权";
  if (source === "profile_dir") return "隔离目录";
  if (source === "local_snapshot") return "本机登录";
  return "额度账号";
}

function waitSeconds(seconds: number, stopped: () => boolean): Promise<void> {
  const total = Math.max(0, seconds) * 1000;
  if (stopped() || total === 0) return Promise.resolve();
  return new Promise((resolve) => {
    const started = Date.now();
    const timer = window.setInterval(() => {
      if (stopped() || Date.now() - started >= total) {
        window.clearInterval(timer);
        resolve();
      }
    }, 200);
  });
}

/**
 * 官方额度下面的额度账号。可以导入当前本机登录，登记一个隔离配置目录。
 * Codex 可以走浏览器授权，Copilot 可以走设备码。界面只显示掩码，不拿令牌。
 */
export function QuotaAccountPanel({ onQuota }: { onQuota: (value: OfficialQuotaDto) => void }) {
  const [panel, setPanel] = useState<QuotaAccountPanelDto | null>(null);
  const [provider, setProvider] = useState<string>(OFFICIAL_QUOTA_PROVIDER_IDS[0]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftName, setDraftName] = useState("");
  const [draftProvider, setDraftProvider] = useState<string>(OFFICIAL_QUOTA_PROVIDER_IDS[0]);
  const [draftDir, setDraftDir] = useState("");
  const [profileName, setProfileName] = useState("");
  const [profileDir, setProfileDir] = useState("");
  const [device, setDevice] = useState<DevicePrompt | null>(null);
  const [codex, setCodex] = useState<CodexPrompt | null>(null);
  const [callbackUrl, setCallbackUrl] = useState("");
  const [copied, setCopied] = useState(false);
  const stopRef = useRef(false);
  const sessionRef = useRef<string | null>(null);
  const kindRef = useRef<"copilot" | "codex" | null>(null);
  const pasteInFlight = useRef(false);
  const occupied = busy || device !== null || codex !== null;

  useEffect(() => {
    void invoke<QuotaAccountPanelDto>("list_quota_accounts")
      .then(setPanel)
      .catch((cause: unknown) => setError(humanStatus(cause)));
    return () => {
      const sessionId = sessionRef.current;
      const kind = kindRef.current;
      if (!sessionId) return;
      stopRef.current = true;
      sessionRef.current = null;
      kindRef.current = null;
      if (kind === "codex") {
        void invoke("cancel_codex_pkce_login", { sessionId });
        return;
      }
      void invoke("cancel_copilot_device_login", { sessionId });
    };
  }, []);

  async function reload(next: QuotaAccountPanelDto) {
    setPanel(next);
    setPendingDeleteId(null);
    setEditingId(null);
    onQuota(await invoke<OfficialQuotaDto>("get_official_quota"));
  }

  async function run(action: () => Promise<QuotaAccountPanelDto>) {
    setBusy(true);
    setError(null);
    try {
      await reload(await action());
    } catch (cause) {
      setError(humanStatus(cause));
    } finally {
      setBusy(false);
    }
  }

  function importCurrent() {
    void run(async () => {
      const saved = await invoke<SavedQuotaAccountDto>("import_local_quota_account", { provider });
      return saved.panel;
    });
  }

  function save(request: SaveQuotaAccount) {
    void run(async () => {
      const saved = await invoke<SavedQuotaAccountDto>("save_quota_account", { request });
      return saved.panel;
    });
  }

  function saveProfile(request: SaveProfileDirAccount) {
    void run(async () => {
      const saved = await invoke<SavedQuotaAccountDto>("save_profile_dir_account", { request });
      if (request.id == null) {
        setProfileName("");
        setProfileDir("");
      }
      return saved.panel;
    });
  }

  function remove(id: string) {
    void run(() => invoke<QuotaAccountPanelDto>("delete_quota_account", { id }));
  }

  async function openVerification(sessionId: string) {
    try {
      await invoke("open_copilot_device_verification", { sessionId });
    } catch (cause) {
      setError(humanStatus(cause));
    }
  }

  async function copyCode(code: string) {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
    } catch (cause) {
      setError(humanStatus(cause));
    }
  }

  async function cancelDevice() {
    stopRef.current = true;
    const sessionId = sessionRef.current;
    sessionRef.current = null;
    kindRef.current = null;
    setDevice(null);
    setCopied(false);
    setError(CANCELLED);
    if (!sessionId) return;
    try {
      await invoke("cancel_copilot_device_login", { sessionId });
    } catch {
      // 会话已经结束就当取消成功。
    }
  }

  async function openCodex(sessionId: string) {
    try {
      await invoke("open_codex_pkce_login", { sessionId });
    } catch (cause) {
      setError(humanStatus(cause));
    }
  }

  async function cancelCodex() {
    stopRef.current = true;
    const sessionId = sessionRef.current;
    sessionRef.current = null;
    kindRef.current = null;
    setCodex(null);
    setCallbackUrl("");
    setError(CODEX_CANCELLED);
    if (!sessionId) return;
    try {
      await invoke("cancel_codex_pkce_login", { sessionId });
    } catch {
      // 会话已经结束就当取消成功。
    }
  }

  async function submitCodexCallback() {
    const sessionId = sessionRef.current;
    if (!sessionId || pasteInFlight.current) return;
    pasteInFlight.current = true;
    setError(null);
    try {
      const saved = await invoke<SavedQuotaAccountDto>("submit_codex_pkce_callback", {
        sessionId,
        callbackUrl,
      });
      stopRef.current = true;
      sessionRef.current = null;
      kindRef.current = null;
      setCodex(null);
      setCallbackUrl("");
      await reload(saved.panel);
    } catch (cause) {
      setError(humanStatus(cause));
    } finally {
      pasteInFlight.current = false;
    }
  }

  async function beginCodexLogin() {
    stopRef.current = false;
    setBusy(true);
    setError(null);
    setCallbackUrl("");
    try {
      const started = await invoke<CodexPkceStart>("start_codex_pkce_login");
      if (stopRef.current) {
        await invoke("cancel_codex_pkce_login", { sessionId: started.session_id });
        return;
      }
      sessionRef.current = started.session_id;
      kindRef.current = "codex";
      setCodex({
        sessionId: started.session_id,
        authorizeUrl: started.authorize_url,
        listening: started.listening,
        pasteHint: started.paste_hint ?? null,
      });
      void openCodex(started.session_id);
      if (!started.listening) return;
      while (!stopRef.current) {
        await waitSeconds(1, () => stopRef.current);
        if (stopRef.current) return;
        let polled: CodexPkcePoll;
        try {
          polled = await invoke<CodexPkcePoll>("poll_codex_pkce_login", {
            sessionId: started.session_id,
          });
        } catch (cause) {
          if (stopRef.current || pasteInFlight.current) return;
          throw cause;
        }
        if (stopRef.current || pasteInFlight.current) return;
        if (polled.status === "authorized") {
          sessionRef.current = null;
          kindRef.current = null;
          setCodex(null);
          setCallbackUrl("");
          await reload(polled.saved.panel);
          return;
        }
      }
    } catch (cause) {
      if (!stopRef.current) {
        sessionRef.current = null;
        kindRef.current = null;
        setCodex(null);
        setError(humanStatus(cause));
      }
    } finally {
      setBusy(false);
    }
  }

  async function beginDeviceLogin() {
    stopRef.current = false;
    setBusy(true);
    setError(null);
    setCopied(false);
    try {
      const started = await invoke<CopilotDeviceStart>("start_copilot_device_login");
      if (stopRef.current) {
        await invoke("cancel_copilot_device_login", { sessionId: started.session_id });
        return;
      }
      sessionRef.current = started.session_id;
      kindRef.current = "copilot";
      let interval = started.interval_seconds;
      setDevice({
        sessionId: started.session_id,
        userCode: started.user_code,
        verificationUrl: started.verification_url,
        intervalSeconds: interval,
      });
      void openVerification(started.session_id);
      while (!stopRef.current) {
        await waitSeconds(interval, () => stopRef.current);
        if (stopRef.current) return;
        let polled: CopilotDevicePoll;
        try {
          polled = await invoke<CopilotDevicePoll>("poll_copilot_device_login", {
            sessionId: started.session_id,
          });
        } catch (cause) {
          if (stopRef.current) return;
          throw cause;
        }
        if (stopRef.current) return;
        if (polled.status === "authorized") {
          sessionRef.current = null;
          kindRef.current = null;
          setDevice(null);
          setCopied(false);
          await reload(polled.saved.panel);
          return;
        }
        interval = polled.interval_seconds;
        setDevice((current) =>
          current ? { ...current, intervalSeconds: interval } : current,
        );
      }
    } catch (cause) {
      if (!stopRef.current) {
        sessionRef.current = null;
        kindRef.current = null;
        setDevice(null);
        setError(humanStatus(cause));
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="official-quota-accounts">
      <div className="panel-head">
        <div>
          <h3>额度账号</h3>
          <p className="panel-note">
            导入当前本机登录，或登记一个已有的配置目录。Codex
            可以用浏览器授权登录，GitHub Copilot
            可以用设备码授权，都不改原客户端的登录态。同一身份再登录一次会更新原来那一条。隔离目录只保存路径，不复制里面的登录。界面只显示掩码。
          </p>
        </div>
      </div>
      <div className="row-actions">
        <label className="field">
          <span className="field-label">提供方</span>
          <select
            value={provider}
            disabled={occupied}
            onChange={(event) => setProvider(event.target.value)}
          >
            {OFFICIAL_QUOTA_PROVIDER_IDS.map((id) => (
              <option key={id} value={id}>
                {officialQuotaProviderLabel(id)}
              </option>
            ))}
          </select>
        </label>
        <Button variant="accent" disabled={occupied} onClick={importCurrent}>
          导入当前本机登录
        </Button>
        {provider === "copilot" ? (
          <Button disabled={occupied} onClick={() => void beginDeviceLogin()}>
            设备码授权登录
          </Button>
        ) : null}
        {provider === "codex" ? (
          <Button disabled={occupied} onClick={() => void beginCodexLogin()}>
            浏览器授权登录
          </Button>
        ) : null}
      </div>
      <div className="custom-quota-form">
        <label className="field">
          <span className="field-label">名称</span>
          <input
            value={profileName}
            aria-label="隔离目录名称"
            onChange={(event) => setProfileName(event.target.value)}
          />
        </label>
        <label className="field">
          <span className="field-label">配置目录</span>
          <input
            value={profileDir}
            aria-label="配置目录"
            placeholder="绝对路径"
            onChange={(event) => setProfileDir(event.target.value)}
          />
        </label>
        <div className="row-actions">
          <Button
            variant="accent"
            disabled={occupied}
            onClick={() =>
              saveProfile({
                id: null,
                provider,
                name: profileName,
                profile_dir: profileDir,
              })
            }
          >
            添加隔离目录
          </Button>
        </div>
      </div>
      {device ? (
        <div className="device-login">
          <p className="panel-note">
            在 GitHub 验证页输入下面的一次性验证码。码表不监听本地端口。当前每 {device.intervalSeconds}{" "}
            秒查询一次，可以随时取消。
          </p>
          <code className="device-code" aria-label="设备验证码">
            {device.userCode}
          </code>
          <code>{device.verificationUrl}</code>
          <div className="row-actions">
            <Button onClick={() => void copyCode(device.userCode)}>
              {copied ? "已复制" : "复制验证码"}
            </Button>
            <Button onClick={() => void openVerification(device.sessionId)}>打开验证页面</Button>
            <Button onClick={() => void cancelDevice()}>取消</Button>
          </div>
        </div>
      ) : null}
      {codex ? (
        <div className="device-login">
          <p className="panel-note">
            {codex.listening
              ? "已在本机临时监听回调。如果浏览器没能回到这台电脑，把地址栏里的回调链接粘贴到下面。可以随时取消。"
              : codex.pasteHint}
          </p>
          <code>{codex.authorizeUrl}</code>
          <label className="field">
            <span className="field-label">回调链接</span>
            <input
              value={callbackUrl}
              aria-label="回调链接"
              placeholder="http://localhost:1455/auth/callback?code=…&state=…"
              onChange={(event) => setCallbackUrl(event.target.value)}
            />
          </label>
          <div className="row-actions">
            <Button onClick={() => void openCodex(codex.sessionId)}>打开授权页面</Button>
            <Button disabled={callbackUrl.trim() === ""} onClick={() => void submitCodexCallback()}>
              使用回调链接
            </Button>
            <Button onClick={() => void cancelCodex()}>取消</Button>
          </div>
        </div>
      ) : null}
      {error ? <p className="panel-note tone-danger">{error}</p> : null}
      {panel && panel.accounts.length === 0 ? (
        <p className="panel-note">还没有导入额度账号。</p>
      ) : null}
      <ul className="custom-quota-list">
        {panel?.accounts.map((account) => (
          <li
            key={account.id}
            className={account.enabled ? "custom-quota-item" : "custom-quota-item is-off"}
          >
            <div className="custom-quota-summary">
              {editingId === account.id ? (
                <input
                  value={draftName}
                  aria-label={`名称「${account.name}」`}
                  onChange={(event) => setDraftName(event.target.value)}
                />
              ) : (
                <strong>{account.name}</strong>
              )}
              {editingId === account.id && account.source === "profile_dir" ? (
                <select
                  aria-label={`提供方「${account.name}」`}
                  value={draftProvider}
                  onChange={(event) => setDraftProvider(event.target.value)}
                >
                  {OFFICIAL_QUOTA_PROVIDER_IDS.map((id) => (
                    <option key={id} value={id}>
                      {officialQuotaProviderLabel(id)}
                    </option>
                  ))}
                </select>
              ) : (
                <span className="muted">{officialQuotaProviderLabel(account.provider)}</span>
              )}
              <span className="muted">{accountSourceLabel(account.source, account.provider)}</span>
              <span className="muted">{account.hint}</span>
              {account.source === "profile_dir" && editingId === account.id ? (
                <input
                  value={draftDir}
                  aria-label={`配置目录「${account.name}」`}
                  onChange={(event) => setDraftDir(event.target.value)}
                />
              ) : account.profile_dir ? (
                <span className="muted">{account.profile_dir}</span>
              ) : null}
              <div className="row-actions">
                {pendingDeleteId === account.id ? (
                  <>
                    <Button variant="danger" disabled={occupied} onClick={() => remove(account.id)}>
                      确认删除
                    </Button>
                    <Button disabled={occupied} onClick={() => setPendingDeleteId(null)}>
                      取消
                    </Button>
                  </>
                ) : editingId === account.id ? (
                  <>
                    <Button
                      variant="accent"
                      disabled={occupied}
                      onClick={() => {
                        if (account.source === "profile_dir") {
                          saveProfile({
                            id: account.id,
                            provider: draftProvider,
                            name: draftName,
                            profile_dir: draftDir,
                          });
                          return;
                        }
                        save({ id: account.id, name: draftName, enabled: null });
                      }}
                    >
                      保存
                    </Button>
                    <Button disabled={occupied} onClick={() => setEditingId(null)}>
                      取消
                    </Button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      className={[
                        "custom-quota-switch",
                        account.enabled ? "is-on" : "is-off",
                      ].join(" ")}
                      role="switch"
                      aria-checked={account.enabled}
                      aria-label={`启用「${account.name}」`}
                      disabled={occupied}
                      onClick={() =>
                        save({ id: account.id, name: null, enabled: !account.enabled })
                      }
                    >
                      {account.enabled ? "已启用" : "已停用"}
                    </button>
                    <Button
                      disabled={occupied}
                      onClick={() => {
                        setPendingDeleteId(null);
                        setEditingId(account.id);
                        setDraftName(account.name);
                        setDraftProvider(account.provider);
                        setDraftDir(account.profile_dir ?? "");
                      }}
                    >
                      {account.source === "profile_dir" ? "编辑" : "改名"}
                    </Button>
                    <Button
                      variant="danger"
                      disabled={occupied}
                      onClick={() => {
                        setEditingId(null);
                        setPendingDeleteId(account.id);
                      }}
                    >
                      删除
                    </Button>
                  </>
                )}
              </div>
              {pendingDeleteId === account.id ? (
                <p className="panel-note tone-warn">这条额度账号会从列表和首页消失，保存的登录也会被清掉。</p>
              ) : null}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
