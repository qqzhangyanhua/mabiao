import type { IconName } from "../icons";
import type { View } from "../types";
import { navLabel } from "./viewTitle";

export type NavItem = {
  id: View;
  icon: IconName;
};

export type NavGroup = {
  label: string;
  items: readonly NavItem[];
};

export const NAV_GROUPS: readonly NavGroup[] = [
  {
    label: "用量",
    items: [
      { id: "overview", icon: "overview" },
      { id: "trend", icon: "trend" },
      { id: "model", icon: "model" },
      { id: "project", icon: "project" },
      { id: "application", icon: "source" },
      { id: "provider", icon: "provider" },
      { id: "worktime", icon: "clock" },
    ],
  },
  {
    label: "对话",
    items: [{ id: "conversations", icon: "chat" }],
  },
  {
    label: "Cursor",
    items: [
      { id: "cursor", icon: "cursor" },
      { id: "cursor-sessions", icon: "sessions" },
    ],
  },
  {
    label: "系统",
    items: [
      { id: "instructions", icon: "instruction" },
      { id: "settings", icon: "settings" },
    ],
  },
];

export const NAV_VIEWS: readonly View[] = NAV_GROUPS.flatMap((group) =>
  group.items.map((item) => item.id),
);

const SHORTCUT_SLOT_COUNT = 10;

export function shortcutKeyAt(index: number): string | null {
  if (index < 0 || index >= SHORTCUT_SLOT_COUNT) {
    return null;
  }
  return index === 9 ? "0" : String(index + 1);
}

export function shortcutKeyForView(view: View): string | null {
  return shortcutKeyAt(NAV_VIEWS.indexOf(view));
}

export function viewForShortcutKey(key: string): View | undefined {
  if (key.length !== 1) {
    return undefined;
  }
  const index = key === "0" ? 9 : Number(key) - 1;
  if (!Number.isInteger(index) || index < 0 || index >= SHORTCUT_SLOT_COUNT) {
    return undefined;
  }
  return NAV_VIEWS[index];
}

export function shortcutRangeLabel(): string {
  const count = Math.min(NAV_VIEWS.length, SHORTCUT_SLOT_COUNT);
  if (count <= 0) {
    return "";
  }
  if (count === 1) {
    return "1";
  }
  return count === SHORTCUT_SLOT_COUNT ? "1-0" : `1-${count}`;
}

export function shortcutLegend(): string {
  return NAV_VIEWS.slice(0, SHORTCUT_SLOT_COUNT)
    .map((view, index) => `${shortcutKeyAt(index)} ${navLabel(view)}`)
    .join(" · ");
}
