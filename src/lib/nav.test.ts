import { describe, expect, it } from "vitest";
import { views } from "../hooks/viewCache";
import {
  NAV_GROUPS,
  NAV_VIEWS,
  shortcutKeyAt,
  shortcutKeyForView,
  shortcutLegend,
  shortcutRangeLabel,
  viewForShortcutKey,
} from "./nav";

describe("NAV_VIEWS", () => {
  it("is the sidebar flattened, not a second shortcut table", () => {
    expect(NAV_VIEWS).toEqual(NAV_GROUPS.flatMap((group) => group.items.map((item) => item.id)));
    expect(NAV_VIEWS).toEqual([
      "overview",
      "trend",
      "model",
      "project",
      "application",
      "provider",
      "worktime",
      "conversations",
      "cursor",
      "cursor-sessions",
      "instructions",
      "settings",
    ]);
  });

  it("covers every View exactly once", () => {
    expect([...NAV_VIEWS].sort()).toEqual([...views].sort());
    expect(new Set(NAV_VIEWS).size).toBe(NAV_VIEWS.length);
  });
});

describe("digit shortcuts", () => {
  it("maps 1-0 onto the first 10 sidebar items", () => {
    expect(viewForShortcutKey("1")).toBe("overview");
    expect(viewForShortcutKey("2")).toBe("trend");
    expect(viewForShortcutKey("3")).toBe("model");
    expect(viewForShortcutKey("4")).toBe("project");
    expect(viewForShortcutKey("5")).toBe("application");
    expect(viewForShortcutKey("6")).toBe("provider");
    expect(viewForShortcutKey("7")).toBe("worktime");
    expect(viewForShortcutKey("8")).toBe("conversations");
    expect(viewForShortcutKey("9")).toBe("cursor");
    expect(viewForShortcutKey("0")).toBe("cursor-sessions");
  });

  it("leaves items past the first 10 without a digit", () => {
    expect(shortcutKeyForView("instructions")).toBeNull();
    expect(shortcutKeyForView("settings")).toBeNull();
    expect(shortcutKeyAt(10)).toBeNull();
  });

  it("ignores non-digit keys", () => {
    expect(viewForShortcutKey("a")).toBeUndefined();
    expect(viewForShortcutKey("10")).toBeUndefined();
    expect(viewForShortcutKey("")).toBeUndefined();
    expect(viewForShortcutKey("-")).toBeUndefined();
  });
});

describe("shortcut footer", () => {
  it("follows the flattened table for the visible range and title legend", () => {
    expect(shortcutRangeLabel()).toBe("1-0");
    expect(shortcutLegend()).toBe(
      "1 概览 · 2 使用统计 · 3 模型统计 · 4 项目统计 · 5 来源统计 · 6 接口统计 · 7 工作时间线 · 8 对话记录 · 9 代码量 · 0 会话",
    );
  });
});
