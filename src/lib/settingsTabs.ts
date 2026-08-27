import type { SettingsTab, SettingsTabId } from "./type";

export const SETTINGS_TABS: readonly SettingsTab[] = [
  { id: "general", label: "通用", anchors: ["settings-appearance"] },
  { id: "sources", label: "数据源", anchors: ["settings-diagnostics", "settings-conversation-index"] },
  { id: "display", label: "展示", anchors: ["settings-overview"] },
  {
    id: "budget",
    label: "预算",
    anchors: ["settings-budget", "settings-official-quota", "settings-custom-quota"],
  },
  { id: "backup", label: "备份", anchors: ["settings-backup"] },
  { id: "cursor", label: "Cursor", anchors: ["settings-cursor-account"] },
  {
    id: "pricing",
    label: "价格",
    anchors: ["settings-litellm", "settings-unpriced", "settings-presets", "settings-prices"],
  },
];

const HASH_ALIASES: Record<string, SettingsTabId> = {
  settings: "general",
  "settings-general": "general",
  "settings-refresh": "general",
  "settings-sources": "sources",
  "settings-display": "display",
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
