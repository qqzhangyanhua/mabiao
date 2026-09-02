import type { SettingsTab, SettingsTabId } from "./type";

export const SETTINGS_UNPRICED_ANCHOR = "settings-unpriced";

export const SETTINGS_TABS: readonly SettingsTab[] = [
  { id: "general", label: "通用", anchors: ["settings-appearance", "settings-overview"] },
  {
    id: "sources",
    label: "数据",
    anchors: [
      "settings-scan-paths",
      "settings-diagnostics",
      "settings-conversation-index",
      "settings-backup",
    ],
  },
  {
    id: "quota",
    label: "额度",
    anchors: ["settings-official-quota", "settings-custom-quota"],
  },
  {
    id: "pricing",
    label: "费用",
    anchors: [
      "settings-budget",
      "settings-litellm",
      SETTINGS_UNPRICED_ANCHOR,
      "settings-presets",
      "settings-prices",
    ],
  },
  { id: "cursor", label: "Cursor", anchors: ["settings-cursor-account"] },
];

const HASH_ALIASES: Record<string, SettingsTabId> = {
  settings: "general",
  "settings-general": "general",
  "settings-refresh": "general",
  "settings-display": "general",
  "settings-sources": "sources",
  "settings-quota": "quota",
  "settings-pricing": "pricing",
};

const ANCHOR_TO_TAB = Object.fromEntries(
  SETTINGS_TABS.flatMap((tab) => tab.anchors.map((anchor) => [anchor, tab.id])),
) as Record<string, SettingsTabId>;

export function tabFromHash(raw: string): SettingsTabId {
  const hash = raw.replace(/^#/, "");
  return HASH_ALIASES[hash] ?? ANCHOR_TO_TAB[hash] ?? "general";
}

export function hashForTab(id: SettingsTabId): string {
  const tab = SETTINGS_TABS.find((item) => item.id === id);
  return tab?.anchors[0] ?? "settings-appearance";
}
