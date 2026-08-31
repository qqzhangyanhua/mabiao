import { describe, expect, it } from "vitest";
import { navLabel, viewTitle } from "./viewTitle";

describe("viewTitle — three session entry points", () => {
  it("keeps 对话记录 as the full-source body catalog", () => {
    const { title, subtitle } = viewTitle("conversations");
    expect(title).toBe("对话记录");
    expect(subtitle).toContain("正文");
    expect(subtitle).toContain("Cursor");
  });

  it("keeps Cursor 会话 as the domain title and states there is no body", () => {
    const { title, subtitle } = viewTitle("cursor-sessions");
    expect(title).toBe("Cursor 会话");
    expect(subtitle).toContain("不含正文");
    expect(subtitle).toContain("对话记录");
  });

  it("describes 工作时间线 and that bars open the same conversation", () => {
    const { title, subtitle } = viewTitle("worktime");
    expect(title).toBe("工作时间线");
    expect(subtitle).toContain("会话");
    expect(subtitle).toContain("对话记录");
  });
});

describe("navLabel — sidebar disambiguation", () => {
  it("keeps domain labels; Cursor entry is 会话 not 行为统计", () => {
    expect(navLabel("conversations")).toBe("对话记录");
    expect(navLabel("cursor-sessions")).toBe("会话");
    expect(navLabel("worktime")).toBe("工作时间线");
  });
});

describe("navLabel — 来源 vs 接口", () => {
  it("calls the local-tool page 来源统计, not 应用统计", () => {
    expect(navLabel("application")).toBe("来源统计");
    expect(viewTitle("application")).toEqual({
      title: "来源统计",
      subtitle: "按本地工具看趋势、交叉与效率",
    });
  });

  it("calls the API page 接口统计, not Provider", () => {
    expect(navLabel("provider")).toBe("接口统计");
    expect(viewTitle("provider")).toEqual({
      title: "接口统计",
      subtitle: "按实际调用的 API 拆分，区分官方与中转",
    });
  });
});
