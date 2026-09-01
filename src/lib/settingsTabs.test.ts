import { describe, expect, it } from "vitest";
import { hashForTab, SETTINGS_TABS, SETTINGS_UNPRICED_ANCHOR, tabFromHash } from "./settingsTabs";

describe("tabFromHash", () => {
  it("maps the settings root and general aliases to the general tab", () => {
    expect(tabFromHash("#settings")).toBe("general");
    expect(tabFromHash("settings-appearance")).toBe("general");
    expect(tabFromHash("#settings-refresh")).toBe("general");
    expect(tabFromHash("#settings-display")).toBe("general");
    expect(tabFromHash("settings-overview")).toBe("general");
  });

  it("keeps existing panel anchors on their grouped tab", () => {
    expect(tabFromHash("#settings-diagnostics")).toBe("sources");
    expect(tabFromHash("#settings-conversation-index")).toBe("sources");
    expect(tabFromHash("#settings-backup")).toBe("sources");
    expect(tabFromHash("#settings-official-quota")).toBe("quota");
    expect(tabFromHash("#settings-custom-quota")).toBe("quota");
    expect(tabFromHash("#settings-budget")).toBe("pricing");
    expect(tabFromHash("#settings-cursor-account")).toBe("cursor");
    expect(tabFromHash("#settings-litellm")).toBe("pricing");
    expect(tabFromHash(`#${SETTINGS_UNPRICED_ANCHOR}`)).toBe("pricing");
    expect(tabFromHash("#settings-presets")).toBe("pricing");
    expect(tabFromHash("#settings-prices")).toBe("pricing");
  });

  it("falls back to general for unknown hashes", () => {
    expect(tabFromHash("#settings-unknown")).toBe("general");
    expect(tabFromHash("")).toBe("general");
  });
});

describe("hashForTab", () => {
  it("uses five semantic tabs", () => {
    expect(SETTINGS_TABS.map((tab) => tab.label)).toEqual([
      "通用",
      "数据",
      "额度",
      "费用",
      "Cursor",
    ]);
  });

  it("writes the first panel anchor for each tab", () => {
    expect(hashForTab("general")).toBe("settings-appearance");
    expect(hashForTab("quota")).toBe("settings-official-quota");
    expect(hashForTab("pricing")).toBe("settings-budget");
  });

  it("covers every declared tab", () => {
    for (const tab of SETTINGS_TABS) {
      expect(hashForTab(tab.id)).toBe(tab.anchors[0]);
      expect(tabFromHash(hashForTab(tab.id))).toBe(tab.id);
    }
  });

  it("keeps the unpriced diagnosis list on the pricing tab", () => {
    const pricing = SETTINGS_TABS.find((tab) => tab.id === "pricing");
    expect(pricing?.anchors).toContain(SETTINGS_UNPRICED_ANCHOR);
    expect(tabFromHash(SETTINGS_UNPRICED_ANCHOR)).toBe("pricing");
  });

  it("does not give budget quota and custom providers the same tab", () => {
    expect(tabFromHash("#settings-budget")).toBe("pricing");
    expect(tabFromHash("#settings-official-quota")).toBe("quota");
    expect(tabFromHash("#settings-custom-quota")).toBe("quota");
  });
});
