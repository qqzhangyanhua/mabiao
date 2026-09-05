import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConversationSessionRow } from "../types";
import {
  capabilityLabel,
  conversationQueryFromFilter,
  conversationSourceLabel,
  conversationDetailSummary,
  conversationFileUnavailableLabel,
  conversationRangeLabel,
  conversationRangeTitle,
  conversationSessionTime,
  conversationSourceOptions,
  conversationSourcesFromUsageFilter,
  conversationStatusLabel,
} from "./conversationDisplay";

function session(overrides: Partial<ConversationSessionRow> = {}): ConversationSessionRow {
  return {
    source: "codex",
    session_id: "conv-1",
    title: "实现折叠",
    project: "/workspace/project",
    model: "gpt-test",
    started_at: "2026-08-21T00:00:00Z",
    ended_at: "2026-08-21T00:01:00Z",
    source_file: "conv-1.jsonl",
    source_files: ["conv-1.jsonl"],
    capabilities: ["messages", "events"],
    support_status: "experimental",
    file_available: true,
    total_tokens: 0,
    cost: null,
    unpriced: false,
    ...overrides,
  };
}

describe("conversation display labels", () => {
  it("maps known capabilities and leaves unknown ids unchanged", () => {
    expect(capabilityLabel("events")).toBe("完整事件");
    expect(capabilityLabel("custom")).toBe("custom");
  });

  it("keeps Cursor Agent grouped with Cursor", () => {
    expect(conversationSourceLabel("cursor_agent")).toBe("Cursor / Cursor Agent");
    expect(conversationSourceLabel("codex")).toBe("Codex");
  });

  it("always offers Cursor Agent in the conversation source list", () => {
    expect(conversationSourceOptions(["claude", "codex"])).toEqual([
      "claude",
      "codex",
      "cursor_agent",
    ]);
    expect(conversationSourceOptions(["cursor_agent"])).toEqual(["cursor_agent"]);
  });

  it("maps usage-page cursor source onto conversation catalog cursor_agent", () => {
    expect(conversationSourcesFromUsageFilter([])).toEqual([]);
    expect(conversationSourcesFromUsageFilter(["claude", "cursor"])).toEqual([
      "claude",
      "cursor_agent",
    ]);
    expect(conversationSourcesFromUsageFilter(["cursor"])).toEqual(["cursor_agent"]);
  });

  it("builds a catalog query from the shared topbar filter", () => {
    expect(
      conversationQueryFromFilter(
        {
          from: "2026-08-01T00:00:00Z",
          to: "2026-08-31T23:59:59Z",
          sources: ["cursor"],
          models: ["opus"],
          projects: ["/old"],
          providers: ["anthropic"],
        },
        { projects: ["/proj/a"], page: 2, page_size: 10 },
      ),
    ).toEqual({
      search: null,
      page: 2,
      page_size: 10,
      sources: ["cursor_agent"],
      projects: ["/proj/a"],
      models: ["opus"],
      providers: ["anthropic"],
      from: "2026-08-01T00:00:00Z",
      to: "2026-08-31T23:59:59Z",
      tool_names: [],
      tool_failed: false,
    });
  });

  it("translates experimental status and file-missing chips", () => {
    expect(conversationStatusLabel("experimental")).toBe("实验性");
    expect(conversationStatusLabel("stable")).toBe("stable");
    expect(conversationFileUnavailableLabel("cursor_agent")).toBe("缺少 transcript");
    expect(conversationFileUnavailableLabel("codex")).toBe("原文件已删除");
  });

  it("prefers ended_at for the session clock", () => {
    expect(conversationSessionTime(session())).toBe("2026-08-21T00:01:00Z");
    expect(conversationSessionTime(session({ ended_at: "" }))).toBe("2026-08-21T00:00:00Z");
  });
});

describe("conversation range", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders start and end as a relative range", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-21T10:01:00Z"));
    expect(conversationRangeLabel(session())).toBe("10 小时前 → 10 小时前");
    expect(conversationRangeTitle(session())).toContain("→");
    expect(conversationRangeLabel(session({ started_at: "", ended_at: "" }))).toBe("—");
  });
});

describe("conversationDetailSummary", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("joins source, project, model and relative time", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-21T10:01:00Z"));
    expect(conversationDetailSummary(session())).toBe("Codex · project · gpt-test · 10 小时前");
  });

  it("falls back when model or project is empty and omits missing time", () => {
    expect(
      conversationDetailSummary(
        session({
          source: "cursor_agent",
          project: "",
          model: "",
          started_at: "",
          ended_at: "",
        }),
      ),
    ).toBe("Cursor / Cursor Agent · 未标注 · 未标注");
  });
});
