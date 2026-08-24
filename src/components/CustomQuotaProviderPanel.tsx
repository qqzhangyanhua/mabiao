import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import {
  BLANK_CUSTOM_QUOTA_DRAFT,
  submittedSecret,
  type CustomQuotaDraft,
  type CustomQuotaPreset,
} from "../lib/customQuotaDraft";
import { humanStatus } from "../lib/format";
import type { OfficialQuotaDto } from "../types";
import { CustomQuotaProviderForm, type CustomQuotaPresetDto } from "./CustomQuotaProviderForm";
import { Button } from "./ui/Button";

/**
 * 自定义提供商的命令契约。只有这个面板用得到，因此就近放在这里——
 * 与 `CursorAccountSettingsPanel` 里的 `CursorCredentialStatus` 同一个办法；
 * `types.ts` 留给跨视图共享的 DTO。字段名保持 snake_case，与 Rust 对齐。
 */
type CustomQuotaProviderDto = {
  /** 形如 `custom:a3f9c1`，随机生成，与内置 9 家永不冲突。改名不改它。 */
  id: string;
  name: string;
  preset: CustomQuotaPreset;
  base_url: string;
  enabled: boolean;
  /** 掩码串；没配密钥（多半是恢复备份后）为 null。永远拿不到明文。 */
  secret_mask: string | null;
};

type CustomQuotaPanelDto = {
  providers: CustomQuotaProviderDto[];
  presets: CustomQuotaPresetDto[];
};

/** 保存请求。id 为 null 表示新建；enabled / secret 留空表示沿用现有值。 */
type SaveCustomQuotaProvider = {
  id: string | null;
  name: string;
  preset: CustomQuotaPreset;
  base_url: string;
  enabled: boolean | null;
  secret: string | null;
};

type SavedCustomQuotaDto = {
  saved_id: string;
  panel: CustomQuotaPanelDto;
};

function draftFrom(provider: CustomQuotaProviderDto): CustomQuotaDraft {
  return {
    id: provider.id,
    name: provider.name,
    preset: provider.preset,
    baseUrl: provider.base_url,
    secret: "",
  };
}

export function CustomQuotaProviderPanel({
  onQuota,
}: {
  onQuota: (value: OfficialQuotaDto) => void;
}) {
  const [panel, setPanel] = useState<CustomQuotaPanelDto | null>(null);
  const [draft, setDraft] = useState<CustomQuotaDraft | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void invoke<CustomQuotaPanelDto>("list_custom_quota_providers")
      .then(setPanel)
      .catch((cause: unknown) => setError(humanStatus(cause)));
  }, []);

  /**
   * 存 / 删之后刷新首页那一块。`refreshId` 有值时额外去取一次那一条的额度：
   * `get_official_quota` 只读缓存，不这么做的话「存完就在首页看到那一行」
   * 会变成「看到一行『暂无』，还得自己再点一次刷新」。
   */
  async function run(action: () => Promise<CustomQuotaPanelDto>, refreshId?: () => string | null) {
    setBusy(true);
    setError(null);
    try {
      const next = await action();
      setPanel(next);
      setDraft(null);
      const id = refreshId?.() ?? null;
      // 取数失败不抛错——错误会写进那一行里，用人话显示出来。
      onQuota(
        id == null
          ? await invoke<OfficialQuotaDto>("get_official_quota")
          : await invoke<OfficialQuotaDto>("refresh_official_quota_provider", { provider: id }),
      );
    } catch (cause) {
      setError(humanStatus(cause));
    } finally {
      setBusy(false);
    }
  }

  function save(current: CustomQuotaDraft) {
    const request: SaveCustomQuotaProvider = {
      id: current.id,
      name: current.name,
      preset: current.preset,
      base_url: current.baseUrl,
      // 本面板还没有启停开关，null 表示「别动现在的状态」。
      enabled: null,
      secret: submittedSecret(current.secret),
    };
    let savedId: string | null = null;
    void run(
      async () => {
        const saved = await invoke<SavedCustomQuotaDto>("save_custom_quota_provider", { request });
        savedId = saved.saved_id;
        return saved.panel;
      },
      () => savedId,
    );
  }

  function remove(id: string, name: string) {
    if (!window.confirm(`删除「${name}」？首页那一行会一起消失，密钥也会被清掉。`)) {
      return;
    }
    void run(() => invoke<CustomQuotaPanelDto>("delete_custom_quota_provider", { id }));
  }

  return (
    <section className="panel" id="settings-custom-quota">
      <div className="panel-head">
        <div>
          <h2>自定义提供商</h2>
          <p className="panel-note">
            第三方中转站、聚合服务的余额。登记之后和内置账号并排出现在首页「官方额度」里。
            密钥单独存一份不进备份的文件，界面上始终掩码显示。
          </p>
        </div>
        <div className="row-actions">
          <Button
            variant="accent"
            disabled={busy || draft != null}
            onClick={() => setDraft({ ...BLANK_CUSTOM_QUOTA_DRAFT })}
          >
            新增
          </Button>
        </div>
      </div>

      {error ? <p className="panel-note tone-danger">{error}</p> : null}

      {panel && panel.providers.length === 0 && draft == null ? (
        <p className="panel-note">还没有登记任何自定义提供商。</p>
      ) : null}

      <ul className="custom-quota-list">
        {panel?.providers.map((provider) => (
          <li key={provider.id} className="custom-quota-item">
            <div className="custom-quota-summary">
              <strong>{provider.name}</strong>
              <span className="muted">{presetLabel(panel.presets, provider.preset)}</span>
              <code>{provider.base_url}</code>
              <span className="muted">{provider.secret_mask ?? "未配置密钥，请重新填写"}</span>
              <div className="row-actions">
                <Button
                  disabled={busy || draft != null}
                  onClick={() => setDraft(draftFrom(provider))}
                >
                  编辑
                </Button>
                <Button
                  variant="danger"
                  disabled={busy || draft != null}
                  onClick={() => remove(provider.id, provider.name)}
                >
                  删除
                </Button>
              </div>
            </div>
          </li>
        ))}
      </ul>

      {draft && panel ? (
        <CustomQuotaProviderForm
          draft={draft}
          presets={panel.presets}
          busy={busy}
          onChange={setDraft}
          onCancel={() => setDraft(null)}
          onSubmit={() => save(draft)}
        />
      ) : null}
    </section>
  );
}

function presetLabel(presets: CustomQuotaPresetDto[], value: CustomQuotaPreset): string {
  return presets.find((preset) => preset.value === value)?.label ?? value;
}
