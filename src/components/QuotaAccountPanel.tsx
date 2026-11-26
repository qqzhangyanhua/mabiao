import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { humanStatus } from "../lib/format";
import { OFFICIAL_QUOTA_PROVIDER_IDS, officialQuotaProviderLabel } from "../lib/overviewLayout";
import type { OfficialQuotaDto } from "../types";
import { Button } from "./ui/Button";

type QuotaAccountSource = "local_snapshot" | "profile_dir";

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

/**
 * 官方额度下面的额度账号。可以导入当前本机登录，也可以登记一个隔离配置目录。
 * 界面只显示掩码，不显示令牌。
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

  useEffect(() => {
    void invoke<QuotaAccountPanelDto>("list_quota_accounts")
      .then(setPanel)
      .catch((cause: unknown) => setError(humanStatus(cause)));
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

  return (
    <div className="official-quota-accounts">
      <div className="panel-head">
        <div>
          <h3>额度账号</h3>
          <p className="panel-note">
            导入当前本机登录，或登记一个已有的配置目录。同一登录再导一次会更新原来那一条。隔离目录只保存路径，不复制里面的登录。界面只显示掩码。
          </p>
        </div>
      </div>
      <div className="row-actions">
        <label className="field">
          <span className="field-label">提供方</span>
          <select value={provider} onChange={(event) => setProvider(event.target.value)}>
            {OFFICIAL_QUOTA_PROVIDER_IDS.map((id) => (
              <option key={id} value={id}>
                {officialQuotaProviderLabel(id)}
              </option>
            ))}
          </select>
        </label>
        <Button variant="accent" disabled={busy} onClick={importCurrent}>
          导入当前本机登录
        </Button>
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
            disabled={busy}
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
              <span className="muted">
                {account.source === "profile_dir" ? "隔离目录" : "本机登录"}
              </span>
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
                    <Button variant="danger" disabled={busy} onClick={() => remove(account.id)}>
                      确认删除
                    </Button>
                    <Button disabled={busy} onClick={() => setPendingDeleteId(null)}>
                      取消
                    </Button>
                  </>
                ) : editingId === account.id ? (
                  <>
                    <Button
                      variant="accent"
                      disabled={busy}
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
                    <Button disabled={busy} onClick={() => setEditingId(null)}>
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
                      disabled={busy}
                      onClick={() =>
                        save({ id: account.id, name: null, enabled: !account.enabled })
                      }
                    >
                      {account.enabled ? "已启用" : "已停用"}
                    </button>
                    <Button
                      disabled={busy}
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
                      disabled={busy}
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
                <p className="panel-note tone-warn">这条账号会从列表和首页消失，保存的登录也会被清掉。</p>
              ) : null}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
