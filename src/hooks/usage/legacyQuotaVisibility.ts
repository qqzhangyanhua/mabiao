import {
  extractLegacyHiddenOfficialProviders,
  OVERVIEW_LAYOUT_STORAGE_KEY,
  stripLegacyOfficialProvidersFromStorage,
} from "../../lib/overviewLayout";
import type { OfficialQuotaConfig, OfficialQuotaConfigPatch, OfficialQuotaDto } from "../../types";

/**
 * 一次性旧版浏览器存储迁移器：若 `localStorage` 里仍留有旧 `officialProviders` 开关，
 * 且持久化 `official_quota.json` 尚未设置 `hidden_providers`，仅在首次读到官方额度时
 * 原子迁移一次并清理旧键；备份恢复时直接清理旧键，防止旧浏览器存储反盖恢复后的配置。
 */
export class LegacyQuotaVisibilityMigrator {
  private checked = false;
  private inFlight: Promise<string[] | null> | null = null;
  private readonly storage: Storage | null;

  constructor(storage: Storage | null) {
    this.storage = storage;
  }

  markRestored(): void {
    this.checked = true;
    stripLegacyOfficialProvidersFromStorage(this.storage);
  }

  async reconcile(
    quota: OfficialQuotaDto,
    patchConfig: (patch: OfficialQuotaConfigPatch) => Promise<OfficialQuotaConfig>,
  ): Promise<OfficialQuotaDto> {
    if (this.checked) {
      return quota;
    }
    const raw = this.readLegacyLayoutRaw();
    const legacy = extractLegacyHiddenOfficialProviders(raw);
    if (!legacy.hasLegacyField) {
      this.checked = true;
      return quota;
    }
    if ((quota.hidden_providers?.length ?? 0) > 0 || legacy.hiddenProviders.length === 0) {
      stripLegacyOfficialProvidersFromStorage(this.storage);
      this.checked = true;
      return quota;
    }
    if (!this.inFlight) {
      this.inFlight = patchConfig({ hidden_providers: legacy.hiddenProviders })
        .then((saved) => {
          stripLegacyOfficialProvidersFromStorage(this.storage);
          this.checked = true;
          return saved.hidden_providers;
        })
        .catch(() => null)
        .finally(() => {
          this.inFlight = null;
        });
    }
    const migrated = await this.inFlight;
    if (migrated === null) {
      return quota;
    }
    return {
      ...quota,
      hidden_providers: migrated,
    };
  }

  private readLegacyLayoutRaw(): string | null {
    if (!this.storage) {
      return null;
    }
    try {
      return this.storage.getItem(OVERVIEW_LAYOUT_STORAGE_KEY);
    } catch {
      return null;
    }
  }
}
