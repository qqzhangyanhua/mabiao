import { describe, expect, it } from "vitest";
import type { ConversationCatalogProgressDto, IngestReport } from "../types";
import {
  catalogFinishedStatus,
  catalogProgressStatus,
  catalogStatus,
  composeStatus,
  ingestFailureStatus,
  isCatalogManagedStatus,
  isCatalogSyncing,
  usageStageStatus,
} from "./ingestStatus";

const report = {
  files_parsed: 3,
  files_skipped: 5,
  records_written: 42,
  records_archived: 0,
  records_removed: 0,
  files_failed: 0,
  partial_success: false,
  conversation_issues: [],
} as unknown as IngestReport;

const progress = (patch: Partial<ConversationCatalogProgressDto>): ConversationCatalogProgressDto => ({
  done: 1,
  total: 4,
  source: "codex",
  finished: false,
  issue_count: 0,
  ...patch,
});

describe("ingestStatus", () => {
  it("formats usage stage completion without conversation issues", () => {
    expect(usageStageStatus("启动摄取", report)).toBe(
      "启动摄取完成：解析 3，跳过 5，写入 42",
    );
  });

  it("adds archive, removal and failure counts and marks partial success", () => {
    const text = usageStageStatus("刷新", {
      ...report,
      records_archived: 2,
      records_removed: 1,
      files_failed: 4,
      partial_success: true,
    });
    expect(text).toBe("刷新部分完成：解析 3，跳过 5，写入 42，归档 2，清理 1，失败 4");
  });

  it("formats catalog progress", () => {
    expect(catalogProgressStatus(progress({ done: 2, total: 7 }))).toBe("对话目录同步中 2/7");
    expect(isCatalogSyncing(progress({}))).toBe(true);
    expect(isCatalogSyncing(progress({ finished: true }))).toBe(false);
    expect(isCatalogSyncing(null)).toBe(false);
  });

  it("keeps the base status when the catalog finishes cleanly", () => {
    expect(catalogFinishedStatus("启动摄取完成：解析 3", 0)).toBe("启动摄取完成：解析 3");
  });

  it("appends the issue count when the catalog finishes with issues", () => {
    expect(catalogFinishedStatus("启动摄取完成：解析 3", 2)).toBe(
      "启动摄取完成：解析 3，对话目录保留 2 项问题",
    );
    expect(catalogFinishedStatus("", 2)).toBe("对话目录保留 2 项问题");
  });

  it("dispatches by finished flag", () => {
    expect(catalogStatus(progress({}), "base")).toBe("对话目录同步中 1/4");
    expect(catalogStatus(progress({ finished: true, issue_count: 1 }), "base")).toBe(
      "base，对话目录保留 1 项问题",
    );
  });

  it("formats failure", () => {
    expect(ingestFailureStatus("启动摄取", "boom")).toBe("启动摄取失败：boom");
  });

  it("composes the usage status with the latest catalog state in either arrival order", () => {
    const usage = "启动摄取完成：解析 3";
    expect(composeStatus(usage, null)).toBe(usage);
    expect(composeStatus(usage, progress({ done: 3, total: 21 }))).toBe("对话目录同步中 3/21");
    expect(composeStatus(usage, progress({ finished: true, issue_count: 0 }))).toBe(usage);
    expect(composeStatus(usage, progress({ finished: true, issue_count: 2 }))).toBe(
      `${usage}，对话目录保留 2 项问题`,
    );
    expect(composeStatus("", progress({ finished: true, issue_count: 2 }))).toBe(
      "对话目录保留 2 项问题",
    );
  });

  it("recognises status text the catalog stage owns and leaves newer unrelated text alone", () => {
    const usage = "启动摄取完成：解析 3";
    expect(isCatalogManagedStatus(usage, usage)).toBe(true);
    expect(isCatalogManagedStatus("对话目录同步中 2/21", usage)).toBe(true);
    expect(isCatalogManagedStatus(`${usage}，对话目录保留 1 项问题`, usage)).toBe(true);
    expect(isCatalogManagedStatus("对话目录保留 1 项问题", "")).toBe(true);
    expect(isCatalogManagedStatus("全部缓存重建中…", usage)).toBe(false);
    expect(isCatalogManagedStatus("启动摄取中…", "")).toBe(false);
    expect(isCatalogManagedStatus("单价已保存", usage)).toBe(false);
    expect(isCatalogManagedStatus("", "")).toBe(false);
  });
});
